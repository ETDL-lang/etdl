//! Live enforcement and observation engine for the ETDL Performance
//! Supplement (`etdl.performance`).
//!
//! Unlike `etdl-core::live` (the Live Reliability Supplement's engine),
//! this module is **always compiled in**, not feature-gated: it needs no
//! dependency beyond `tokio`'s `time`/`sync` features, which this crate
//! already pulls in unconditionally. Generated code only ever calls into
//! it when a document declares `etdl.performance` and a Budget applies to
//! the node being compiled — see `docs/reference/performance-supplement.md`.
//!
//! Three requirement kinds, three different treatments (see the
//! supplement's own Section 6):
//!
//! - **Concurrency** (`maxConcurrency`) — a [`tokio::sync::Semaphore`]
//!   sized to the declared limit. [`enter`] blocks (a real, unconditional
//!   wait, not advisory) until a permit is available — the number of
//!   concurrent guarded calls can never exceed the declared limit, by
//!   construction.
//! - **Throughput** (`expectedRatePerSecond`) — a small hand-rolled async
//!   token bucket (no existing crate dependency needed for this). `enter`
//!   waits for a token to become available, smoothing bursts down to the
//!   declared rate rather than rejecting them outright.
//! - **Latency** (`p50Ms`/`p95Ms`/`p99Ms`) — cannot be enforced before a
//!   call runs, only observed. Every guarded call's total duration
//!   (including any time spent waiting for capacity above) is recorded
//!   into a bounded rolling window; [`in_budget`] compares its current
//!   percentiles against the declared ceilings.
//!
//! [`in_budget`] folds all three into one boolean — the value
//! `performance.in_budget` (ECEL) resolves to for a Barrier linked to a
//! Budget via `x-performance.barrierChecks` — fail-open (`true`) with
//! insufficient data or an unregistered budget id, the same convention
//! `etdl_core::sla`/`etdl_core::live` both already use.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How many observations one `in_budget` decision needs before it's
/// meaningful — mirrors `etdl_core::sla::SlaTracker`'s own
/// `MIN_OBSERVATIONS` convention ("insufficient data => not an anomaly"),
/// scaled down since a performance observation is a real timed async call,
/// not a cheap in-memory record — a smaller minimum keeps tests (and real
/// warm-up periods) fast without weakening the guarantee's intent.
const MIN_LATENCY_OBSERVATIONS: usize = 5;

/// Bounded rolling window size for latency observations — same default
/// `SlaTracker::DEFAULT_WINDOW_SIZE` uses.
const LATENCY_WINDOW_SIZE: usize = 1000;

/// How far back "the observed rate" looks when `in_budget` compares it
/// against `expectedRatePerSecond`.
const RATE_WINDOW: Duration = Duration::from_secs(1);

/// A Budget's declared requirements — the parts `etdl_core::perf` acts on.
/// Mirrors `etdl_compiler::performance::Budget`'s numeric fields exactly;
/// this type has no dependency on the compiler crate, so generated code
/// constructs one directly from the literals codegen already has.
#[derive(Debug, Clone, Copy)]
pub struct BudgetSpec {
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_concurrency: Option<u32>,
    pub expected_rate_per_second: Option<f64>,
}

struct TokenBucket {
    tokens: f64,
    capacity: f64,
    rate_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    /// Starts full (`capacity` tokens available immediately) — an initial
    /// burst up to the declared rate is allowed, then smoothed.
    fn new(rate_per_sec: f64) -> Self {
        TokenBucket {
            tokens: rate_per_sec,
            capacity: rate_per_sec,
            rate_per_sec,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate_per_sec).min(self.capacity);
        self.last_refill = now;
    }

    /// `None` if a token was available and consumed now; `Some(wait)` — how
    /// long until one will be — otherwise.
    fn try_acquire(&mut self) -> Option<Duration> {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            None
        } else {
            let deficit = 1.0 - self.tokens;
            Some(Duration::from_secs_f64(deficit / self.rate_per_sec))
        }
    }
}

async fn acquire_token(bucket: &Mutex<TokenBucket>) {
    loop {
        let wait = bucket.lock().unwrap().try_acquire();
        match wait {
            None => return,
            Some(d) => tokio::time::sleep(d).await,
        }
    }
}

struct PerfState {
    spec: BudgetSpec,
    semaphore: Option<Arc<Semaphore>>,
    rate_limiter: Option<Mutex<TokenBucket>>,
    latency_window_ms: Mutex<VecDeque<f64>>,
    /// Timestamps of recent guarded-call completions, pruned to
    /// [`RATE_WINDOW`] on read — the observed-rate half of `in_budget`,
    /// independent of the token bucket used for enforcement above (that
    /// bucket's own internal token count isn't a meaningful "requests in
    /// the last second" figure once bursts have been smoothed).
    rate_window: Mutex<VecDeque<Instant>>,
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<PerfState>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<PerfState>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lookup(budget_id: &str) -> Option<Arc<PerfState>> {
    registry().lock().unwrap().get(budget_id).cloned()
}

/// Registers a Budget's requirements under `budget_id`, ready for
/// [`enter`]/[`in_budget`]. Idempotent in the sense that a second call
/// simply overwrites the first (generated code wraps this in a
/// `std::sync::Once` so it only ever runs once per process per budget —
/// see `codegen/rust.rs`'s `generate_performance_registration`).
pub fn register_budget(budget_id: &str, spec: BudgetSpec) {
    let semaphore = spec
        .max_concurrency
        .map(|n| Arc::new(Semaphore::new(n as usize)));
    let rate_limiter = spec
        .expected_rate_per_second
        .map(|r| Mutex::new(TokenBucket::new(r)));
    let state = Arc::new(PerfState {
        spec,
        semaphore,
        rate_limiter,
        latency_window_ms: Mutex::new(VecDeque::new()),
        rate_window: Mutex::new(VecDeque::new()),
    });
    registry().lock().unwrap().insert(budget_id.to_string(), state);
}

/// Held for the duration of a Budget-guarded call. Dropping it (or calling
/// [`PerfGuard::finish`] explicitly) records the elapsed time since
/// [`enter`] returned into the budget's rolling latency window and
/// releases any concurrency permit held. Recording happens exactly once
/// regardless of which of the two ways the guard's lifetime ends —
/// `finish` and `Drop` share the same private, idempotent path.
pub struct PerfGuard {
    state: Option<Arc<PerfState>>,
    _permit: Option<OwnedSemaphorePermit>,
    start: Instant,
    recorded: bool,
}

impl PerfGuard {
    /// Explicit, precisely-placed recording point — used at the single
    /// point an Operation's guarded call has actually finished (codegen
    /// emits this right after the `retry.execute(...).await` it wraps,
    /// before branching into `Ok`/`Err`), where relying on `Drop` timing
    /// instead would be less clear about exactly when the call ended.
    pub fn finish(mut self) {
        self.record();
    }

    fn record(&mut self) {
        if self.recorded {
            return;
        }
        self.recorded = true;
        let Some(state) = &self.state else { return };

        let elapsed_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        {
            let mut window = state.latency_window_ms.lock().unwrap();
            window.push_back(elapsed_ms);
            if window.len() > LATENCY_WINDOW_SIZE {
                window.pop_front();
            }
        }
        {
            let mut rate_window = state.rate_window.lock().unwrap();
            rate_window.push_back(Instant::now());
            prune_rate_window(&mut rate_window);
        }
    }
}

impl Drop for PerfGuard {
    /// Covers the whole-Event-Tree case (`codegen/rust.rs` emits a
    /// top-of-handler guard with no explicit `finish()` call, since the
    /// handler has multiple exit points — an Operation's non-retried
    /// `Err(err) => return Err(...)` arm in particular) — Drop always
    /// runs, on every exit path, success or error.
    fn drop(&mut self) {
        self.record();
    }
}

fn prune_rate_window(window: &mut VecDeque<Instant>) {
    let cutoff = Instant::now() - RATE_WINDOW;
    while window.front().is_some_and(|t| *t < cutoff) {
        window.pop_front();
    }
}

/// Begins a Budget-guarded call: waits for a concurrency permit and a rate
/// token (whichever the budget declares) before returning, so by the time
/// the caller proceeds to actually run the guarded code, both requirements
/// are already satisfied — a real, unconditional wait, not advisory. An
/// unregistered `budget_id` (should not happen for generated code, which
/// always registers before use, but is not a compiler-enforced invariant
/// this function trusts blindly) returns immediately with an inert guard
/// that records nothing.
pub async fn enter(budget_id: &str) -> PerfGuard {
    let start = Instant::now();
    let Some(state) = lookup(budget_id) else {
        return PerfGuard {
            state: None,
            _permit: None,
            start,
            recorded: false,
        };
    };

    let permit = match &state.semaphore {
        Some(sem) => Some(
            Arc::clone(sem)
                .acquire_owned()
                .await
                .expect("this crate never calls Semaphore::close"),
        ),
        None => None,
    };

    if let Some(bucket) = &state.rate_limiter {
        acquire_token(bucket).await;
    }

    PerfGuard {
        state: Some(state),
        _permit: permit,
        start,
        recorded: false,
    }
}

/// Whether `budget_id`'s requirements currently appear to be met: every
/// declared percentile is at or under its ceiling, concurrency has not
/// saturated `maxConcurrency` (if declared), and the observed rate over
/// the last second has not exceeded `expectedRatePerSecond` (if declared).
/// `true` (fail-open) for an unregistered budget id or with fewer than
/// [`MIN_LATENCY_OBSERVATIONS`] latency samples so far — the same
/// "insufficient data is not an anomaly" convention
/// `etdl_core::sla::SlaTracker`/`etdl_core::live::in_range` both use.
pub fn in_budget(budget_id: &str) -> bool {
    let Some(state) = lookup(budget_id) else {
        return true;
    };

    if let Some(sem) = &state.semaphore {
        if sem.available_permits() == 0 {
            return false;
        }
    }

    if let Some(expected) = state.spec.expected_rate_per_second {
        let mut window = state.rate_window.lock().unwrap();
        prune_rate_window(&mut window);
        if window.len() as f64 > expected {
            return false;
        }
    }

    let samples: Vec<f64> = {
        let window = state.latency_window_ms.lock().unwrap();
        if window.len() < MIN_LATENCY_OBSERVATIONS {
            return true;
        }
        window.iter().copied().collect()
    };

    let mut sorted = samples;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let percentile = |p: f64| -> f64 {
        let idx = (((sorted.len() - 1) as f64) * p).round() as usize;
        sorted[idx]
    };

    percentile(0.50) <= state.spec.p50_ms
        && percentile(0.95) <= state.spec.p95_ms
        && percentile(0.99) <= state.spec.p99_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The registry is process-wide (`static REGISTRY`), shared across
    /// every test in this binary under the default parallel test runner —
    /// distinct ids per test are what keeps them independent.
    fn unique_id(name: &str) -> String {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        format!("{name}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    fn unlimited_spec() -> BudgetSpec {
        BudgetSpec {
            p50_ms: 1000.0,
            p95_ms: 1000.0,
            p99_ms: 1000.0,
            max_concurrency: None,
            expected_rate_per_second: None,
        }
    }

    #[test]
    fn in_budget_is_true_for_an_unregistered_budget() {
        assert!(in_budget("no-such-budget"));
    }

    #[tokio::test]
    async fn in_budget_fails_open_with_insufficient_observations() {
        let id = unique_id("cold-start");
        register_budget(&id, unlimited_spec());
        assert!(in_budget(&id));
        // One observation is still below MIN_LATENCY_OBSERVATIONS.
        enter(&id).await.finish();
        assert!(in_budget(&id));
    }

    #[tokio::test]
    async fn in_budget_flips_false_once_enough_slow_observations_land() {
        let id = unique_id("slow-latency");
        register_budget(
            &id,
            BudgetSpec {
                // A ceiling low enough that any real sleep-based call blows it.
                p50_ms: 1.0,
                p95_ms: 1.0,
                p99_ms: 1.0,
                max_concurrency: None,
                expected_rate_per_second: None,
            },
        );
        for _ in 0..MIN_LATENCY_OBSERVATIONS {
            let guard = enter(&id).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            guard.finish();
        }
        assert!(!in_budget(&id));
    }

    #[tokio::test]
    async fn finish_and_drop_both_record_but_never_double_record() {
        let id = unique_id("finish-vs-drop");
        register_budget(
            &id,
            BudgetSpec {
                p50_ms: 1.0,
                p95_ms: 1.0,
                p99_ms: 1.0,
                max_concurrency: None,
                expected_rate_per_second: None,
            },
        );
        for _ in 0..MIN_LATENCY_OBSERVATIONS {
            // Explicit finish().
            let guard = enter(&id).await;
            tokio::time::sleep(Duration::from_millis(5)).await;
            guard.finish();
        }
        // If finish() didn't prevent Drop from recording again, the window
        // would have 2x MIN_LATENCY_OBSERVATIONS entries — harmless to
        // in_budget's boolean here, but exactly the double-count this test
        // exists to rule out.
        let state = lookup(&id).unwrap();
        let count = state.latency_window_ms.lock().unwrap().len();
        assert_eq!(count, MIN_LATENCY_OBSERVATIONS);
    }

    #[tokio::test]
    async fn concurrency_guard_never_exceeds_max_concurrency() {
        let id = unique_id("concurrency");
        register_budget(
            &id,
            BudgetSpec {
                max_concurrency: Some(2),
                ..unlimited_spec()
            },
        );

        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..6 {
            let id = id.clone();
            let current = Arc::clone(&current);
            let max_seen = Arc::clone(&max_seen);
            handles.push(tokio::spawn(async move {
                let guard = enter(&id).await;
                let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await;
                current.fetch_sub(1, Ordering::SeqCst);
                guard.finish();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert!(
            max_seen.load(Ordering::SeqCst) <= 2,
            "observed {} concurrent guarded calls against a limit of 2",
            max_seen.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn in_budget_is_false_while_concurrency_is_saturated() {
        let id = unique_id("concurrency-in-budget");
        register_budget(
            &id,
            BudgetSpec {
                max_concurrency: Some(1),
                ..unlimited_spec()
            },
        );

        let held = enter(&id).await;
        assert!(!in_budget(&id), "the only permit is currently held");
        held.finish();
        assert!(in_budget(&id), "the permit was released");
    }

    #[tokio::test]
    async fn rate_limiter_throttles_beyond_expected_rate() {
        let id = unique_id("rate");
        register_budget(
            &id,
            BudgetSpec {
                expected_rate_per_second: Some(10.0),
                ..unlimited_spec()
            },
        );

        // Drain the initial burst capacity (a fresh TokenBucket starts
        // full, per TokenBucket::new's doc comment).
        for _ in 0..10 {
            enter(&id).await.finish();
        }

        let start = Instant::now();
        enter(&id).await.finish();
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(80),
            "expected the 11th call (bucket drained) to wait roughly 1/10s \
             for a new token, got {elapsed:?}"
        );
    }
}
