//! Portable HTTP, retry, cancellation, and cache primitives for Ferro applications.
//!
//! Applications run blocking transports on their own worker threads, then use an
//! [`ferro_sdk::AppEventProxy`] to deliver completed work back to a native runtime.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ferro_sdk::{FerroError, FerroErrorKind};

/// An immutable HTTP request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpRequest {
    /// HTTPS endpoint to request.
    pub url: String,
    /// Additional request headers.
    pub headers: BTreeMap<String, String>,
}

impl HttpRequest {
    /// Builds a GET request for `url`.
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
        }
    }
}

/// A completed HTTP response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    /// Numeric HTTP status code.
    pub status: u16,
    /// Response headers, normalized by the transport where possible.
    pub headers: BTreeMap<String, String>,
    /// Raw response body.
    pub body: Vec<u8>,
}

/// Synchronous transport boundary. Execute it from a worker thread.
pub trait HttpTransport: Send + Sync {
    /// Executes one request.
    ///
    /// # Errors
    ///
    /// Returns a service error when the transport cannot complete the request.
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, FerroError>;
}

/// HTTPS transport implemented with `reqwest` and Rustls.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    /// Creates a transport with the supplied per-request timeout.
    ///
    /// # Errors
    ///
    /// Returns a service error if the client cannot be configured.
    pub fn new(timeout: Duration) -> Result<Self, FerroError> {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .https_only(true)
            .build()
            .map(|client| Self { client })
            .map_err(|error| {
                FerroError::new(
                    FerroErrorKind::Service,
                    format!("could not configure HTTP client: {error}"),
                )
            })
    }
}

impl HttpTransport for ReqwestTransport {
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, FerroError> {
        let mut builder = self.client.get(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder.send().map_err(|error| {
            FerroError::new(
                FerroErrorKind::Service,
                format!("HTTP request failed: {error}"),
            )
        })?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
            })
            .collect();
        let body = response
            .bytes()
            .map_err(|error| {
                FerroError::new(
                    FerroErrorKind::Service,
                    format!("could not read HTTP response: {error}"),
                )
            })?
            .to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

/// Cooperative cancellation shared with a worker request.
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Marks the associated work as cancelled.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    /// Returns whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Bounded retry behavior for idempotent requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    /// Total attempts including the initial request.
    pub max_attempts: u8,
    /// Delay before the second attempt.
    pub initial_delay: Duration,
    /// Maximum `Retry-After` accepted from a server.
    pub max_retry_after: Duration,
}

impl RetryPolicy {
    /// Weather tutorial default: three attempts, then fail visibly.
    #[must_use]
    pub const fn weather_default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            max_retry_after: Duration::from_secs(30),
        }
    }

    /// Returns whether an HTTP status may be retried.
    #[must_use]
    pub const fn retries_status(status: u16) -> bool {
        matches!(status, 429 | 500 | 502 | 503 | 504)
    }

    /// Calculates exponential backoff, honoring a bounded server hint.
    #[must_use]
    pub fn delay_for(self, retry_index: u8, retry_after: Option<Duration>) -> Duration {
        if let Some(delay) = retry_after {
            return delay.min(self.max_retry_after);
        }
        self.initial_delay
            .saturating_mul(2_u32.saturating_pow(u32::from(retry_index)))
    }
}

/// Explicit freshness policy for persisted cache records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CachePolicy {
    /// Maximum age served without a revalidation request.
    pub fresh_for: Duration,
    /// Maximum age served while a revalidation request is attempted.
    pub stale_for: Duration,
}

impl CachePolicy {
    /// Weather tutorial default: fresh for ten minutes, usable stale for one day.
    #[must_use]
    pub const fn weather_default() -> Self {
        Self {
            fresh_for: Duration::from_secs(600),
            stale_for: Duration::from_hours(24),
        }
    }
    /// Classifies a record by its age.
    #[must_use]
    pub fn classify(self, age: Duration) -> CacheState {
        if age <= self.fresh_for {
            CacheState::Fresh
        } else if age <= self.stale_for {
            CacheState::Stale
        } else {
            CacheState::Missing
        }
    }
}

/// Freshness state returned by [`CachePolicy`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheState {
    /// The cached value is current.
    Fresh,
    /// The value may be rendered while it is revalidated.
    Stale,
    /// The value is expired and must not be rendered.
    Missing,
}

/// A value plus the UNIX-millisecond instant at which it was obtained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheEntry<T> {
    /// Cached application value.
    pub value: T,
    /// UNIX-millisecond timestamp at which `value` was fetched.
    pub fetched_at_ms: u64,
}

/// Runs an idempotent request with bounded retries.
///
/// # Errors
///
/// Returns cancellation, transport, or final response errors without hiding the last failure.
pub fn execute_with_retry<T: HttpTransport>(
    transport: &T,
    request: &HttpRequest,
    policy: RetryPolicy,
    cancellation: &CancellationToken,
) -> Result<HttpResponse, FerroError> {
    let attempts = policy.max_attempts.max(1);
    let mut last_error = None;
    for attempt in 0..attempts {
        if cancellation.is_cancelled() {
            return Err(FerroError::new(
                FerroErrorKind::Service,
                "HTTP request cancelled",
            ));
        }
        match transport.execute(request) {
            Ok(response) if !RetryPolicy::retries_status(response.status) => return Ok(response),
            Ok(response) => {
                let retry_after = response
                    .headers
                    .get("retry-after")
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(Duration::from_secs);
                last_error = Some(FerroError::new(
                    FerroErrorKind::Service,
                    format!("HTTP request returned {}", response.status),
                ));
                if attempt + 1 < attempts {
                    std::thread::sleep(policy.delay_for(attempt, retry_after));
                }
            }
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    std::thread::sleep(policy.delay_for(attempt, None));
                }
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| FerroError::new(FerroErrorKind::Service, "HTTP request failed")))
}

#[cfg(test)]
mod tests {
    use super::{CachePolicy, CacheState, RetryPolicy};
    use std::time::Duration;

    #[test]
    fn weather_cache_is_fresh_then_stale_then_missing() {
        let policy = CachePolicy::weather_default();
        assert_eq!(policy.classify(Duration::from_secs(600)), CacheState::Fresh);
        assert_eq!(policy.classify(Duration::from_secs(601)), CacheState::Stale);
        assert_eq!(
            policy.classify(Duration::from_secs(86_401)),
            CacheState::Missing
        );
    }

    #[test]
    fn retry_policy_only_retries_transient_statuses() {
        assert!(RetryPolicy::retries_status(429));
        assert!(RetryPolicy::retries_status(503));
        assert!(!RetryPolicy::retries_status(400));
    }
}
