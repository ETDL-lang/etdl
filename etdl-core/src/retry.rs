use std::fmt;
use std::future::Future;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackoffStrategy {
    Fixed,
    Exponential,
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub strategy: BackoffStrategy,
}

/// The failure mode of a retried operation once attempts are exhausted.
#[derive(Debug, PartialEq)]
pub enum RetryError<E> {
    /// The handler returned `Err` on its final attempt.
    Exhausted(E),
    /// Every attempt timed out (or no attempt was made with `max_attempts == 0`),
    /// so no handler error is available.
    TimedOut,
}

impl<E: fmt::Display> fmt::Display for RetryError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RetryError::Exhausted(e) => write!(f, "retry exhausted: {}", e),
            RetryError::TimedOut => write!(f, "retry exhausted: all attempts timed out"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RetryError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RetryError::Exhausted(e) => Some(e),
            RetryError::TimedOut => None,
        }
    }
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, backoff_ms: u64, strategy: BackoffStrategy) -> Self {
        RetryPolicy {
            max_attempts,
            backoff_ms,
            strategy,
        }
    }

    /// Run `f` up to `max_attempts` times, each under `timeout`.
    ///
    /// - The first `Ok` is returned immediately.
    /// - `Err` is retained as the last error and retried.
    /// - A timeout is recorded and retried (no error value is produced by a
    ///   timeout).
    ///
    /// If attempts are exhausted with a captured handler error, that error is
    /// returned as [`RetryError::Exhausted`]. If they are exhausted with only
    /// timeouts (or `max_attempts == 0`), [`RetryError::TimedOut`] is returned.
    /// This method never panics.
    pub async fn execute<F, Fut, T, E>(
        &self,
        mut f: F,
        timeout: Duration,
    ) -> Result<T, RetryError<E>>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let mut last_error: Option<E> = None;

        for attempt in 0..self.max_attempts {
            match tokio::time::timeout(timeout, f()).await {
                Ok(Ok(result)) => return Ok(result),
                Ok(Err(err)) => {
                    last_error = Some(err);
                }
                Err(_elapsed) => {
                    eprintln!("[etdl] retry attempt {} timed out", attempt + 1);
                }
            }

            if attempt < self.max_attempts - 1 {
                let delay = Duration::from_millis(self.delay_ms(attempt));
                tokio::time::sleep(delay).await;
            }
        }

        match last_error {
            Some(err) => Err(RetryError::Exhausted(err)),
            None => Err(RetryError::TimedOut),
        }
    }

    /// Compute the backoff delay (ms) before attempt `attempt`, saturating so a
    /// document-controlled `max_attempts`/`backoff_ms` can never overflow.
    fn delay_ms(&self, attempt: u32) -> u64 {
        match self.strategy {
            BackoffStrategy::Fixed => self.backoff_ms,
            BackoffStrategy::Exponential => {
                // 2^attempt saturates at u64::MAX for attempt >= 64.
                let factor = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
                self.backoff_ms.saturating_mul(factor)
            }
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 1,
            backoff_ms: 0,
            strategy: BackoffStrategy::Fixed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_first_ok() {
        let policy = RetryPolicy::new(3, 1, BackoffStrategy::Fixed);
        let mut calls = 0;
        let result = policy
            .execute(
                || {
                    calls += 1;
                    async move {
                        if calls == 1 {
                            Err("first")
                        } else {
                            Ok(42)
                        }
                    }
                },
                Duration::from_millis(100),
            )
            .await;
        assert_eq!(result, Ok(42));
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn returns_exhausted_with_last_error() {
        let policy = RetryPolicy::new(2, 1, BackoffStrategy::Fixed);
        let result: Result<i32, RetryError<&str>> = policy
            .execute(
                || async { Err::<i32, &str>("boom") },
                Duration::from_millis(100),
            )
            .await;
        match result {
            Err(RetryError::Exhausted(e)) => assert_eq!(e, "boom"),
            other => panic!("expected Exhausted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn returns_timed_out_when_all_timeout() {
        let policy = RetryPolicy::new(2, 1, BackoffStrategy::Fixed);
        let result: Result<i32, RetryError<&str>> = policy
            .execute(
                || async {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    Ok::<i32, &str>(1)
                },
                Duration::from_millis(1),
            )
            .await;
        assert!(matches!(result, Err(RetryError::TimedOut)));
    }

    #[tokio::test]
    async fn zero_attempts_is_timed_out_not_panic() {
        let policy = RetryPolicy::new(0, 0, BackoffStrategy::Fixed);
        let result: Result<i32, RetryError<&str>> = policy
            .execute(|| async { Ok(1) }, Duration::from_millis(1))
            .await;
        assert!(matches!(result, Err(RetryError::TimedOut)));
    }

    #[test]
    fn exponential_backoff_saturates() {
        let policy = RetryPolicy::new(100, u64::MAX, BackoffStrategy::Exponential);
        // attempt 70: 2^70 overflows u64 -> saturates.
        let d = policy.delay_ms(70);
        assert_eq!(d, u64::MAX);
    }
}
