//! Stable C ABI over `etdl-core` — the one authoritative ETDL runtime.
//!
//! Every language target (`etdl-target-java`, `etdl-target-python`,
//! `etdl-target-go`, `etdl-target-dotnet`) generates thin, language-native
//! bindings to the functions in this crate. None of them re-implement
//! branch/SLA accounting, retry backoff, or ECEL `matches`/`in` semantics —
//! those stay here, in Rust, exactly once. See
//! `docs/architecture/targets.md`.
//!
//! # Ownership model
//!
//! - Every `Etdl*New`/`etdl_*_new` function returns an **opaque handle**
//!   (a boxed Rust value behind a raw pointer) that the caller owns and
//!   must release with the matching `_free` function exactly once. A
//!   handle is never valid to use after `_free`, and never valid to
//!   `_free` twice (both are undefined behavior, same as `free()` in C).
//! - No function in this crate exposes a Rust struct's memory layout
//!   directly — every cross-boundary value is either a primitive
//!   (`i32`/`u32`/`u64`/`f64`/`bool`), a null-terminated UTF-8 C string, or
//!   an opaque pointer. This is what makes the ABI stable across Rust
//!   compiler/etdl-core versions that don't change these function
//!   signatures.
//! - Every exported function is **not** safe to call concurrently on the
//!   *same* handle from multiple threads without external synchronization
//!   (matching `etdl-core`'s own types); creating one handle per
//!   invocation (one `BranchMonitor` per event-tree call, exactly as
//!   generated Rust code already does today) sidesteps this entirely.
//! - **No panic crosses this boundary.** Every exported function body runs
//!   inside [`guard`], which catches any Rust panic and converts it into a
//!   safe sentinel return value (see each function's return-code
//!   documentation) plus a message retrievable via
//!   [`etdl_last_error_message`]. The inverse direction — a foreign
//!   language's own exception/error escaping *its* callback before
//!   returning control to Rust — is that language binding's
//!   responsibility (documented per-target); this crate treats a callback
//!   that can't produce a valid return code as equivalent to "fatal,
//!   do not retry" (see [`etdl_retry_policy_execute`]).
//! - **No timeout enforcement crosses this boundary.** Safely
//!   preempting/cancelling an in-flight call into unknown foreign-language
//!   code from another thread is not implementable without that
//!   language's cooperation, so [`etdl_retry_policy_execute`] provides the
//!   authoritative attempt-count/backoff *sequence* only; a target wanting
//!   a hard per-attempt timeout applies it natively around its own
//!   callback body (e.g. Java's `Future.get(timeout, …)`, exactly as
//!   `etdl-target-java` already did before this crate existed).

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::Mutex;

// ---------------------------------------------------------------------
// ABI version and status codes
// ---------------------------------------------------------------------

/// Bumped only when an existing exported function's signature or semantics
/// change incompatibly. Additive changes (new functions) do not require a
/// bump. A language binding should call [`etdl_runtime_abi_version`] at
/// startup and refuse to run (or warn loudly) against an unexpected major
/// version, rather than silently misinterpreting the ABI.
pub const ETDL_RUNTIME_ABI_VERSION: u32 = 1;

pub const ETDL_OK: i32 = 0;
pub const ETDL_ERR_NULL_HANDLE: i32 = -1;
pub const ETDL_ERR_INVALID_ARG: i32 = -2;
pub const ETDL_ERR_PANIC: i32 = -99;

/// [`etdl_retry_policy_execute`]-specific outcomes (distinct range so a
/// caller can't confuse them with the general status codes above).
pub const ETDL_RETRY_OK: i32 = 0;
pub const ETDL_RETRY_EXHAUSTED: i32 = 1;
pub const ETDL_RETRY_FATAL: i32 = 2;
pub const ETDL_RETRY_INVALID_ARG: i32 = -2;

static VERSION_CSTR: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

/// The `etdl-runtime-ffi` crate (semver) version, as a static, null-terminated
/// UTF-8 C string. Distinct from [`etdl_runtime_abi_version`]: this is for
/// diagnostics/logging, not compatibility checks. Do not free the result —
/// it has static lifetime.
#[no_mangle]
pub extern "C" fn etdl_runtime_version() -> *const c_char {
    VERSION_CSTR.as_ptr() as *const c_char
}

/// See [`ETDL_RUNTIME_ABI_VERSION`].
#[no_mangle]
pub extern "C" fn etdl_runtime_abi_version() -> u32 {
    ETDL_RUNTIME_ABI_VERSION
}

// ---------------------------------------------------------------------
// Panic safety + last-error reporting
// ---------------------------------------------------------------------

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_last_error(message: impl Into<String>) {
    let message = message.into();
    let c_message = CString::new(message.replace('\0', "")).unwrap_or_default();
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(c_message));
}

/// Returns the last error message set on the *calling thread*, as an owned
/// string the caller must release with [`etdl_string_free`], or `NULL` if
/// no function called from this thread has failed yet. Reading it does not
/// clear it — the next failing call on this thread overwrites it.
#[no_mangle]
pub extern "C" fn etdl_last_error_message() -> *mut c_char {
    LAST_ERROR.with(|slot| match slot.borrow().as_ref() {
        Some(msg) => msg.clone().into_raw(),
        None => ptr::null_mut(),
    })
}

/// Frees a string previously returned by this crate (currently only
/// [`etdl_last_error_message`]). Passing any other pointer, or freeing the
/// same pointer twice, is undefined behavior — same contract as C's
/// `free()`. A `NULL` argument is a documented no-op.
#[no_mangle]
pub extern "C" fn etdl_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    let _ = guard((), || unsafe {
        drop(CString::from_raw(s));
    });
}

/// Runs `f`, catching any Rust panic so it can never unwind across the FFI
/// boundary (unwinding into C/Java/Python/Go/.NET is undefined behavior).
/// On panic, records a last-error message and returns `default`.
fn guard<F: FnOnce() -> R, R>(default: R, f: F) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic in etdl-runtime-ffi (no message)".to_string());
            set_last_error(format!("internal panic: {message}"));
            default
        }
    }
}

/// Borrows a `*const c_char` as `&str`, returning `None` (and setting the
/// last-error message) for a null pointer or invalid UTF-8.
fn cstr_to_str<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        set_last_error("argument: null pointer where a C string was expected");
        return None;
    }
    match unsafe { CStr::from_ptr(s) }.to_str() {
        Ok(s) => Some(s),
        Err(e) => {
            set_last_error(format!("argument: invalid UTF-8 ({e})"));
            None
        }
    }
}

// ---------------------------------------------------------------------
// Logging callback — a safe, minimal demonstration of the callback pattern
// used more substantially by `etdl_retry_policy_execute` below.
// ---------------------------------------------------------------------

/// `level`: caller-defined severity (this crate only ever emits `1` = info,
/// `2` = error, from [`etdl_branch_monitor_record_failure`]). `message` is
/// borrowed for the duration of the call only — the callback must copy it
/// if it needs to outlive the call.
///
/// `"C-unwind"`, not plain `"C"`: a genuine foreign-language callback
/// (Java/Python/Go/.NET) never unwinds across this boundary at all — their
/// own exception mechanisms are invisible to Rust's unwinder — so this
/// only changes behavior for the one case that matters for safety testing
/// and for a callback that happens to be implemented in Rust itself: it
/// lets a panic there unwind up to [`guard`]'s `catch_unwind` instead of
/// immediately aborting the process at the callback's own ABI boundary
/// (which is what plain `"C"` mandates since Rust 1.71). See
/// `retry_policy_execute_survives_callback_panic` in this module's tests.
pub type EtdlLogCallback = extern "C-unwind" fn(level: i32, message: *const c_char);

static LOG_CALLBACK: Mutex<Option<EtdlLogCallback>> = Mutex::new(None);

/// Registers (or, with `NULL`, clears) a process-wide log callback invoked
/// for runtime events worth surfacing in the host language's own logging —
/// currently, failures recorded via [`etdl_branch_monitor_record_failure`].
/// A panic inside the callback is caught here and never propagates.
#[no_mangle]
// The callback parameter is spelled out inline (not via the `EtdlLogCallback`
// alias) because cbindgen only niche-optimizes `Option<extern "C-unwind"
// fn(..)>` into a plain nullable C function pointer when it sees the
// function type directly at the call site — through a `pub type` alias it
// instead (in this cbindgen version) emits an opaque
// `Option_EtdlLogCallback` wrapper struct, which is not usable from
// C/Java/Go/.NET. `EtdlLogCallback` stays as the Rust-side ergonomic name
// used everywhere else (tests, doc comments); only the two exported
// functions' own signatures avoid it.
pub extern "C" fn etdl_set_log_callback(callback: Option<extern "C-unwind" fn(level: i32, message: *const c_char)>) {
    guard((), || {
        if let Ok(mut slot) = LOG_CALLBACK.lock() {
            *slot = callback;
        }
    });
}

fn emit_log(level: i32, message: &str) {
    let Ok(slot) = LOG_CALLBACK.lock() else {
        return;
    };
    let Some(callback) = *slot else {
        return;
    };
    if let Ok(c_message) = CString::new(message) {
        let _ = catch_unwind(AssertUnwindSafe(|| callback(level, c_message.as_ptr())));
    }
}

// ---------------------------------------------------------------------
// BranchMonitor
// ---------------------------------------------------------------------

/// Opaque handle wrapping a real `etdl_core::BranchMonitor` — the same
/// type generated Rust code has always used. See the module-level
/// "Ownership model" section: create one per event-tree invocation, free
/// it with [`etdl_branch_monitor_free`] when that invocation completes.
pub struct EtdlBranchMonitor {
    inner: etdl_core::BranchMonitor,
}

#[no_mangle]
pub extern "C" fn etdl_branch_monitor_new(node_id: *const c_char) -> *mut EtdlBranchMonitor {
    guard(ptr::null_mut(), || {
        let Some(node_id) = cstr_to_str(node_id) else {
            return ptr::null_mut();
        };
        Box::into_raw(Box::new(EtdlBranchMonitor {
            inner: etdl_core::BranchMonitor::new(node_id),
        }))
    })
}

/// Frees a handle created by [`etdl_branch_monitor_new`]. `NULL` is a
/// documented no-op.
#[no_mangle]
pub extern "C" fn etdl_branch_monitor_free(handle: *mut EtdlBranchMonitor) {
    if handle.is_null() {
        return;
    }
    guard((), || unsafe {
        drop(Box::from_raw(handle));
    });
}

/// Records that `outcome` was taken with `probability`. Returns
/// [`ETDL_OK`] or a negative error code.
#[no_mangle]
pub extern "C" fn etdl_branch_monitor_record_branch(
    handle: *mut EtdlBranchMonitor,
    outcome: *const c_char,
    probability: f64,
) -> i32 {
    guard(ETDL_ERR_PANIC, || {
        let Some(monitor) = (unsafe { handle.as_mut() }) else {
            set_last_error("etdl_branch_monitor_record_branch: null handle");
            return ETDL_ERR_NULL_HANDLE;
        };
        let Some(outcome) = cstr_to_str(outcome) else {
            return ETDL_ERR_INVALID_ARG;
        };
        monitor.inner.record_branch(outcome, probability);
        ETDL_OK
    })
}

/// Records that `operation_id` completed successfully (its
/// `onFailureProbabilitySource`-linked probability, if any, in
/// `probability` — set `has_probability = false` when the operation has no
/// linked fault tree). Must be paired with
/// [`etdl_branch_monitor_record_failure`] on the same `operation_id` for
/// SLA accounting to reflect the operation's true success/failure mix, not
/// only its failures — see `etdl_core::BranchMonitor::record_success`'s
/// doc comment for why.
#[no_mangle]
pub extern "C" fn etdl_branch_monitor_record_success(
    handle: *mut EtdlBranchMonitor,
    operation_id: *const c_char,
    probability: f64,
    has_probability: bool,
) -> i32 {
    guard(ETDL_ERR_PANIC, || {
        let Some(monitor) = (unsafe { handle.as_mut() }) else {
            set_last_error("etdl_branch_monitor_record_success: null handle");
            return ETDL_ERR_NULL_HANDLE;
        };
        let Some(operation_id) = cstr_to_str(operation_id) else {
            return ETDL_ERR_INVALID_ARG;
        };
        monitor
            .inner
            .record_success(operation_id, has_probability.then_some(probability));
        ETDL_OK
    })
}

/// Records that `operation_id` failed with `error_message`. Also invokes
/// the registered log callback (see [`etdl_set_log_callback`]) with
/// `level = 2`.
#[no_mangle]
pub extern "C" fn etdl_branch_monitor_record_failure(
    handle: *mut EtdlBranchMonitor,
    operation_id: *const c_char,
    error_message: *const c_char,
    probability: f64,
    has_probability: bool,
) -> i32 {
    guard(ETDL_ERR_PANIC, || {
        let Some(monitor) = (unsafe { handle.as_mut() }) else {
            set_last_error("etdl_branch_monitor_record_failure: null handle");
            return ETDL_ERR_NULL_HANDLE;
        };
        let Some(operation_id) = cstr_to_str(operation_id) else {
            return ETDL_ERR_INVALID_ARG;
        };
        let Some(error_message) = cstr_to_str(error_message) else {
            return ETDL_ERR_INVALID_ARG;
        };
        let error = SimpleError(error_message.to_string());
        monitor
            .inner
            .record_failure(operation_id, &error, has_probability.then_some(probability));
        emit_log(2, &format!("{operation_id}: {error_message}"));
        ETDL_OK
    })
}

#[derive(Debug)]
struct SimpleError(String);

impl std::fmt::Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SimpleError {}

// ---------------------------------------------------------------------
// RetryPolicy
// ---------------------------------------------------------------------

pub struct EtdlRetryPolicy {
    inner: etdl_core::RetryPolicy,
}

/// `strategy`: `0` = fixed, `1` = exponential. Any other value is an
/// error (returns `NULL`; see [`etdl_last_error_message`]).
#[no_mangle]
pub extern "C" fn etdl_retry_policy_new(
    max_attempts: u32,
    backoff_ms: u64,
    strategy: i32,
) -> *mut EtdlRetryPolicy {
    guard(ptr::null_mut(), || {
        let strategy = match strategy {
            0 => etdl_core::BackoffStrategy::Fixed,
            1 => etdl_core::BackoffStrategy::Exponential,
            other => {
                set_last_error(format!(
                    "etdl_retry_policy_new: invalid strategy {other} (expected 0=fixed, 1=exponential)"
                ));
                return ptr::null_mut();
            }
        };
        Box::into_raw(Box::new(EtdlRetryPolicy {
            inner: etdl_core::RetryPolicy::new(max_attempts, backoff_ms, strategy),
        }))
    })
}

#[no_mangle]
pub extern "C" fn etdl_retry_policy_free(handle: *mut EtdlRetryPolicy) {
    if handle.is_null() {
        return;
    }
    guard((), || unsafe {
        drop(Box::from_raw(handle));
    });
}

/// The authoritative backoff delay (ms) before `attempt` (0-indexed),
/// exactly the formula `etdl_core::RetryPolicy::execute` itself uses —
/// exposed directly so a target's native retry loop (or
/// [`etdl_retry_policy_execute`] below) never re-derives it.
#[no_mangle]
pub extern "C" fn etdl_retry_policy_delay_ms(handle: *const EtdlRetryPolicy, attempt: u32) -> u64 {
    guard(0, || {
        let Some(policy) = (unsafe { handle.as_ref() }) else {
            set_last_error("etdl_retry_policy_delay_ms: null handle");
            return 0;
        };
        policy.inner.delay_ms(attempt)
    })
}

/// `user_data` is passed through unchanged; `attempt` is the 0-indexed
/// attempt number. Must return:
/// - `0` — this attempt succeeded; the loop stops and
///   [`etdl_retry_policy_execute`] returns [`ETDL_RETRY_OK`].
/// - a negative value — fatal, non-retryable (e.g. the callback caught and
///   is reporting a foreign-language exception it cannot safely continue
///   from); the loop stops immediately and returns [`ETDL_RETRY_FATAL`].
/// - a positive value — this attempt failed but is retryable; the loop
///   sleeps for [`etdl_retry_policy_delay_ms`] (unless this was the last
///   attempt) and tries again.
///
/// The callback itself must not let a panic/exception/error in the
/// caller's own language escape back across this boundary — catch it on
/// the language side and translate it to a negative return instead (see
/// each target's generated binding for how). If it happens anyway (a Rust
/// panic reaching straight through, e.g. from a callback implemented in
/// Rust), [`etdl_retry_policy_execute`] catches it and treats it the same
/// as a negative return.
/// `"C-unwind"`, not plain `"C"` — see [`EtdlLogCallback`]'s doc comment
/// for why.
pub type EtdlRetryCallback = extern "C-unwind" fn(user_data: *mut c_void, attempt: u32) -> i32;

/// Runs the authoritative ETDL retry loop: calls `callback` up to
/// `max_attempts` times (as configured on `handle`), sleeping
/// [`etdl_retry_policy_delay_ms`] between attempts, stopping on the first
/// success or fatal result. See [`EtdlRetryCallback`] for the callback
/// contract. `out_attempts_used` (if non-null) receives how many attempts
/// were actually made.
///
/// No per-attempt timeout is enforced here — see the module-level
/// "Ownership model" section for why; apply one natively around the
/// callback body on the language side if needed.
///
/// Returns [`ETDL_RETRY_OK`], [`ETDL_RETRY_EXHAUSTED`] (every attempt
/// returned a retryable failure), [`ETDL_RETRY_FATAL`] (callback returned
/// negative, or panicked), or [`ETDL_RETRY_INVALID_ARG`] (null
/// handle/callback).
#[no_mangle]
// See `etdl_set_log_callback`'s comment: spelled out inline, not via the
// `EtdlRetryCallback` alias, so cbindgen emits a plain nullable C function
// pointer instead of an opaque wrapper struct.
pub extern "C" fn etdl_retry_policy_execute(
    handle: *const EtdlRetryPolicy,
    callback: Option<extern "C-unwind" fn(user_data: *mut c_void, attempt: u32) -> i32>,
    user_data: *mut c_void,
    out_attempts_used: *mut u32,
) -> i32 {
    guard(ETDL_RETRY_INVALID_ARG, || {
        let Some(policy) = (unsafe { handle.as_ref() }) else {
            set_last_error("etdl_retry_policy_execute: null handle");
            return ETDL_RETRY_INVALID_ARG;
        };
        let Some(callback) = callback else {
            set_last_error("etdl_retry_policy_execute: null callback");
            return ETDL_RETRY_INVALID_ARG;
        };

        let user_data = SendPtr(user_data);
        let max_attempts = policy.inner.max_attempts;
        let mut attempts_used = 0u32;

        for attempt in 0..max_attempts {
            attempts_used = attempt + 1;
            let outcome = catch_unwind(AssertUnwindSafe(|| callback(user_data.0, attempt)));

            match outcome {
                Ok(0) => {
                    write_attempts_used(out_attempts_used, attempts_used);
                    return ETDL_RETRY_OK;
                }
                Ok(rc) if rc < 0 => {
                    set_last_error(format!(
                        "etdl_retry_policy_execute: callback reported a fatal error on attempt {attempt} (code {rc})"
                    ));
                    write_attempts_used(out_attempts_used, attempts_used);
                    return ETDL_RETRY_FATAL;
                }
                Ok(_retryable) => {
                    if attempt + 1 < max_attempts {
                        std::thread::sleep(std::time::Duration::from_millis(
                            policy.inner.delay_ms(attempt),
                        ));
                    }
                }
                Err(_) => {
                    set_last_error(format!(
                        "etdl_retry_policy_execute: retry callback panicked on attempt {attempt}"
                    ));
                    write_attempts_used(out_attempts_used, attempts_used);
                    return ETDL_RETRY_FATAL;
                }
            }
        }

        write_attempts_used(out_attempts_used, attempts_used);
        ETDL_RETRY_EXHAUSTED
    })
}

fn write_attempts_used(out: *mut u32, value: u32) {
    if !out.is_null() {
        unsafe {
            *out = value;
        }
    }
}

/// `*mut c_void` is not `Send` by default (the compiler has no way to know
/// what it points to), but it only ever needs to travel unmodified from
/// this function's caller to `callback` on the very same thread — never
/// actually shared across threads — so wrapping it here is sound and lets
/// the retry-loop closure passed to `catch_unwind` capture it without
/// `catch_unwind` itself needing `Send` (it doesn't; `Send` is not what
/// `AssertUnwindSafe` requires — this wrapper exists purely so the
/// `for`-loop's repeated closure captures compile, since a bare raw
/// pointer capture is `Copy` but this makes the intent explicit).
#[derive(Clone, Copy)]
struct SendPtr(*mut c_void);

// ---------------------------------------------------------------------
// ECEL condition helpers — the same engine `etdl_core::condition` gives
// generated Rust code, so `matches`/`in` behave identically no matter
// which target evaluates them.
// ---------------------------------------------------------------------

/// ECEL `matches` (RE2-compatible regular expression). Returns `1` (match),
/// `0` (no match), or `-1` (invalid argument — see
/// [`etdl_last_error_message`]). An invalid regex pattern is treated as
/// "no match" (`0`), exactly like `etdl_core::condition::matches` itself,
/// not as an error.
#[no_mangle]
pub extern "C" fn etdl_condition_matches(value: *const c_char, pattern: *const c_char) -> i32 {
    guard(-1, || {
        let Some(value) = cstr_to_str(value) else {
            return -1;
        };
        let Some(pattern) = cstr_to_str(pattern) else {
            return -1;
        };
        if etdl_core::condition::matches(value, pattern) {
            1
        } else {
            0
        }
    })
}

/// ECEL `in`. `needle_json` is a single JSON value; `haystack_json_array`
/// is a JSON array. Returns `1` (`needle` is an element of `haystack`),
/// `0` (it is not), or `-1` (invalid/unparseable JSON — see
/// [`etdl_last_error_message`]).
#[no_mangle]
pub extern "C" fn etdl_condition_contains(
    needle_json: *const c_char,
    haystack_json_array: *const c_char,
) -> i32 {
    guard(-1, || {
        let Some(needle_json) = cstr_to_str(needle_json) else {
            return -1;
        };
        let Some(haystack_json) = cstr_to_str(haystack_json_array) else {
            return -1;
        };
        let needle: serde_json::Value = match serde_json::from_str(needle_json) {
            Ok(v) => v,
            Err(e) => {
                set_last_error(format!("etdl_condition_contains: invalid needle JSON: {e}"));
                return -1;
            }
        };
        let haystack: Vec<serde_json::Value> = match serde_json::from_str(haystack_json) {
            Ok(v) => v,
            Err(e) => {
                set_last_error(format!(
                    "etdl_condition_contains: invalid haystack JSON array: {e}"
                ));
                return -1;
            }
        };
        if etdl_core::condition::contains(&haystack, &needle) {
            1
        } else {
            0
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};

    fn cstring(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    #[test]
    fn version_and_abi_version_are_readable() {
        let v = etdl_runtime_abi_version();
        assert_eq!(v, ETDL_RUNTIME_ABI_VERSION);
        let ptr = etdl_runtime_version();
        let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(s, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn branch_monitor_lifecycle_and_error_paths() {
        let node_id = cstring("TestBarrier");
        let handle = etdl_branch_monitor_new(node_id.as_ptr());
        assert!(!handle.is_null());

        let outcome = cstring("SUCCESS");
        let rc = etdl_branch_monitor_record_branch(handle, outcome.as_ptr(), 0.95);
        assert_eq!(rc, ETDL_OK);

        let op = cstring("checkout");
        let rc = etdl_branch_monitor_record_success(handle, op.as_ptr(), 0.05, true);
        assert_eq!(rc, ETDL_OK);

        let msg = cstring("boom");
        let rc = etdl_branch_monitor_record_failure(handle, op.as_ptr(), msg.as_ptr(), 0.05, true);
        assert_eq!(rc, ETDL_OK);

        // Null handle is a documented error, not a crash.
        let rc = etdl_branch_monitor_record_branch(ptr::null_mut(), outcome.as_ptr(), 0.5);
        assert_eq!(rc, ETDL_ERR_NULL_HANDLE);

        etdl_branch_monitor_free(handle);
        etdl_branch_monitor_free(ptr::null_mut()); // no-op, must not crash
    }

    #[test]
    fn retry_policy_delay_ms_matches_documented_formula() {
        let handle = etdl_retry_policy_new(5, 100, 1 /* exponential */);
        assert!(!handle.is_null());
        assert_eq!(etdl_retry_policy_delay_ms(handle, 0), 100);
        assert_eq!(etdl_retry_policy_delay_ms(handle, 1), 200);
        assert_eq!(etdl_retry_policy_delay_ms(handle, 2), 400);
        etdl_retry_policy_free(handle);
    }

    #[test]
    fn retry_policy_new_rejects_invalid_strategy() {
        let handle = etdl_retry_policy_new(3, 10, 99);
        assert!(handle.is_null());
    }

    extern "C-unwind" fn succeed_on_second_attempt(user_data: *mut c_void, attempt: u32) -> i32 {
        let counter = unsafe { &*(user_data as *const AtomicI32) };
        counter.fetch_add(1, Ordering::SeqCst);
        if attempt == 1 {
            0
        } else {
            1
        }
    }

    #[test]
    fn retry_policy_execute_retries_then_succeeds() {
        let handle = etdl_retry_policy_new(5, 1, 0 /* fixed */);
        let counter = AtomicI32::new(0);
        let mut attempts_used = 0u32;
        let rc = etdl_retry_policy_execute(
            handle,
            Some(succeed_on_second_attempt),
            &counter as *const _ as *mut c_void,
            &mut attempts_used,
        );
        assert_eq!(rc, ETDL_RETRY_OK);
        assert_eq!(attempts_used, 2);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        etdl_retry_policy_free(handle);
    }

    extern "C-unwind" fn always_retryable(_user_data: *mut c_void, _attempt: u32) -> i32 {
        1
    }

    #[test]
    fn retry_policy_execute_reports_exhausted() {
        let handle = etdl_retry_policy_new(3, 0, 0);
        let mut attempts_used = 0u32;
        let rc = etdl_retry_policy_execute(
            handle,
            Some(always_retryable),
            ptr::null_mut(),
            &mut attempts_used,
        );
        assert_eq!(rc, ETDL_RETRY_EXHAUSTED);
        assert_eq!(attempts_used, 3);
        etdl_retry_policy_free(handle);
    }

    extern "C-unwind" fn always_fatal(_user_data: *mut c_void, _attempt: u32) -> i32 {
        -1
    }

    #[test]
    fn retry_policy_execute_stops_immediately_on_fatal() {
        let handle = etdl_retry_policy_new(10, 0, 0);
        let mut attempts_used = 0u32;
        let rc = etdl_retry_policy_execute(
            handle,
            Some(always_fatal),
            ptr::null_mut(),
            &mut attempts_used,
        );
        assert_eq!(rc, ETDL_RETRY_FATAL);
        assert_eq!(attempts_used, 1);
        etdl_retry_policy_free(handle);
    }

    extern "C-unwind" fn panics(_user_data: *mut c_void, _attempt: u32) -> i32 {
        panic!("simulated foreign callback misbehavior");
    }

    #[test]
    fn retry_policy_execute_survives_callback_panic() {
        let handle = etdl_retry_policy_new(3, 0, 0);
        let mut attempts_used = 0u32;
        let rc =
            etdl_retry_policy_execute(handle, Some(panics), ptr::null_mut(), &mut attempts_used);
        assert_eq!(rc, ETDL_RETRY_FATAL);
        let err_ptr = etdl_last_error_message();
        assert!(!err_ptr.is_null());
        let msg = unsafe { CStr::from_ptr(err_ptr) }.to_str().unwrap().to_string();
        assert!(msg.contains("panicked"), "got: {msg}");
        etdl_string_free(err_ptr);
        etdl_retry_policy_free(handle);
    }

    #[test]
    fn condition_matches_reuses_etdl_core_regex_semantics() {
        let value = cstring("ORD-12345678");
        let pattern = cstring(r"^ORD-[0-9]{8}$");
        assert_eq!(etdl_condition_matches(value.as_ptr(), pattern.as_ptr()), 1);

        let bad = cstring("order-1");
        assert_eq!(etdl_condition_matches(bad.as_ptr(), pattern.as_ptr()), 0);
    }

    #[test]
    fn condition_contains_reuses_etdl_core_semantics() {
        let needle = cstring("\"PAID\"");
        let haystack = cstring("[\"PAID\", \"AUTHORIZED\"]");
        assert_eq!(
            etdl_condition_contains(needle.as_ptr(), haystack.as_ptr()),
            1
        );

        let missing = cstring("\"REFUNDED\"");
        assert_eq!(
            etdl_condition_contains(missing.as_ptr(), haystack.as_ptr()),
            0
        );

        let bad_json = cstring("not json");
        assert_eq!(
            etdl_condition_contains(needle.as_ptr(), bad_json.as_ptr()),
            -1
        );
    }

    #[test]
    fn last_error_message_is_none_when_nothing_failed_on_this_thread() {
        std::thread::spawn(|| {
            let ptr = etdl_last_error_message();
            assert!(ptr.is_null());
        })
        .join()
        .unwrap();
    }
}
