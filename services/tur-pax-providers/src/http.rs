// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The HTTP transport shared by the adapters.
//!
//! One blocking POST that returns the response body as a reader, with bounded
//! retries. This is the only module that reads the API key text, and the only
//! one that turns the HTTP client's errors into [`ProviderError`]s. Those never
//! carry the client's own error text: it is replaced by a fixed description, so
//! nothing from a request (a header, the URL) can end up in an error.
//!
//! Redirects are not followed. A request carries a key and a conversation, and
//! neither is sent anywhere the configuration did not name.

use std::io::{self, Read};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ureq::http::{HeaderMap, HeaderValue};
use zeroize::Zeroizing;

use crate::config::{ProviderConfig, RetryPolicy};
use crate::error::{ErrorKind, ProviderError};
use crate::model::ProviderId;
use crate::secret::{ApiKey, REDACTED};

/// The body of a successful response.
pub(crate) type BodyReader = Box<dyn Read + Send>;

/// Turns an HTTP error status and the response body into a typed error.
pub(crate) type Classifier = fn(u16, &str) -> ProviderError;

/// The most bytes read from the body of an error response.
const ERROR_BODY_LIMIT: u64 = 64 * 1024;

/// The longest `retry-after` taken at face value, in seconds: one day.
const RETRY_AFTER_MAX_SECONDS: f64 = 86_400.0;

const USER_AGENT: &str = concat!("tur-pax-providers/", env!("CARGO_PKG_VERSION"));

/// How the key is sent.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Auth {
    /// As the value of the named header.
    Header(&'static str),
    /// As `Authorization: Bearer <key>`.
    Bearer,
}

/// One POST.
pub(crate) struct Call<'a> {
    /// The provider, recorded in errors.
    pub(crate) provider: ProviderId,
    /// The full URL.
    pub(crate) url: &'a str,
    /// How the key is sent.
    pub(crate) auth: Auth,
    /// Further fixed headers.
    pub(crate) headers: &'a [(&'static str, &'static str)],
    /// The JSON body.
    pub(crate) body: &'a str,
    /// Maps an error status to a typed error.
    pub(crate) classify: Classifier,
}

/// A configured HTTP client with a retry policy.
#[derive(Clone)]
pub(crate) struct Transport {
    agent: ureq::Agent,
    retry: RetryPolicy,
}

impl Transport {
    /// A transport with the timeouts and retry policy of a configuration.
    pub(crate) fn new(config: &ProviderConfig) -> Self {
        let timeouts = config.timeouts;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .user_agent(USER_AGENT)
            .timeout_connect(Some(Duration::from_millis(timeouts.connect_ms)))
            .timeout_recv_response(Some(Duration::from_millis(timeouts.response_ms)))
            .timeout_per_read(Some(Duration::from_millis(timeouts.idle_ms)))
            .build()
            .into();
        Transport {
            agent,
            retry: config.retry,
        }
    }

    /// Send the request and return the body of a successful response.
    ///
    /// A failure the provider documents as transient is retried, up to the
    /// configured number of attempts, after the delay the provider asked for or
    /// an exponential backoff. Any other failure is returned at once.
    pub(crate) fn open(&self, call: &Call<'_>, key: &ApiKey) -> Result<BodyReader, ProviderError> {
        let mut attempt: u32 = 1;
        loop {
            let error = match self.attempt(call, key) {
                Ok(reader) => return Ok(reader),
                Err(error) => error,
            };
            let Some(delay) = retry_delay(&self.retry, &error, attempt) else {
                return Err(error);
            };
            std::thread::sleep(delay);
            attempt += 1;
        }
    }

    fn attempt(&self, call: &Call<'_>, key: &ApiKey) -> Result<BodyReader, ProviderError> {
        let secret = key.expose();
        let (name, value) = match call.auth {
            Auth::Header(name) => (name, Zeroizing::new(secret.to_owned())),
            Auth::Bearer => ("authorization", Zeroizing::new(format!("Bearer {secret}"))),
        };
        let mut credential = HeaderValue::from_str(&value).map_err(|_| {
            ProviderError::new(ErrorKind::KeyUnavailable)
                .with_detail("the key cannot be sent as an HTTP header")
                .with_provider(call.provider)
        })?;
        // Keeps the value out of the HTTP client's debug output.
        credential.set_sensitive(true);

        let mut request = self
            .agent
            .post(call.url)
            .header(name, credential)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream");
        for (name, value) in call.headers {
            request = request.header(*name, *value);
        }
        let response = request
            .send(call.body)
            .map_err(|error| transport_error(&error).with_provider(call.provider))?;

        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(Box::new(response.into_body().into_reader()));
        }
        let delay = retry_after(response.headers());
        let text = response
            .into_body()
            .into_with_config()
            .limit(ERROR_BODY_LIMIT)
            .lossy_utf8(true)
            .read_to_string()
            .unwrap_or_default();
        // A provider may repeat the key it was sent. It is removed before
        // anything is made of the body.
        let text = text.replace(secret, REDACTED);
        Err((call.classify)(status, &text)
            .with_provider(call.provider)
            .with_status(status)
            .with_retry_after(delay)
            .scrubbed(secret))
    }
}

/// The delay before the next attempt, or `None` if there is to be none.
fn retry_delay(policy: &RetryPolicy, error: &ProviderError, attempt: u32) -> Option<Duration> {
    if !error.is_retryable() || attempt >= policy.max_attempts {
        return None;
    }
    let ceiling = Duration::from_millis(policy.max_delay_ms);
    match error.retry_after() {
        // The provider named a delay and an earlier attempt would fail again.
        // A delay beyond the ceiling is reported to the caller, not shortened.
        Some(delay) if delay > ceiling => None,
        Some(delay) => Some(delay),
        None => Some(backoff(policy, attempt)),
    }
}

/// Exponential backoff with up to a quarter of jitter, capped at the ceiling.
fn backoff(policy: &RetryPolicy, attempt: u32) -> Duration {
    let doublings = attempt.saturating_sub(1).min(20);
    let base = policy
        .initial_delay_ms
        .saturating_mul(1u64 << doublings)
        .min(policy.max_delay_ms);
    let spread = base / 4;
    let jitter = if spread == 0 {
        0
    } else {
        clock_nanos() % (spread + 1)
    };
    Duration::from_millis(base.saturating_add(jitter).min(policy.max_delay_ms))
}

/// The sub-second part of the clock. Enough to spread retries; not random.
fn clock_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::from(elapsed.subsec_nanos()))
        .unwrap_or(0)
}

/// The `retry-after` header as a delay. Only the seconds form is read.
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let text = headers.get("retry-after")?.to_str().ok()?;
    parse_retry_after(text)
}

fn parse_retry_after(text: &str) -> Option<Duration> {
    let seconds: f64 = text.trim().parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(seconds.min(RETRY_AFTER_MAX_SECONDS)))
}

/// The error kind for an HTTP status when the body says nothing more precise.
pub(crate) fn kind_for_status(status: u16) -> ErrorKind {
    match status {
        401 | 403 => ErrorKind::AuthenticationFailed,
        402 => ErrorKind::QuotaExhausted,
        408 | 504 => ErrorKind::TimedOut,
        429 => ErrorKind::RateLimited,
        500..=503 | 505..=599 => ErrorKind::Overloaded,
        _ => ErrorKind::RequestRejected,
    }
}

/// A typed error for a failure of the HTTP client before or while it received
/// a response.
fn transport_error(error: &ureq::Error) -> ProviderError {
    match error {
        ureq::Error::Timeout(_) => ProviderError::new(ErrorKind::TimedOut),
        ureq::Error::Io(source) => io_error_kind(source.kind()),
        ureq::Error::HostNotFound => ProviderError::new(ErrorKind::Network)
            .with_detail("the host name could not be resolved"),
        ureq::Error::ConnectionFailed => ProviderError::new(ErrorKind::Network)
            .with_detail("the connection could not be established"),
        ureq::Error::BadUri(_) | ureq::Error::Http(_) | ureq::Error::RequireHttpsOnly(_) => {
            ProviderError::new(ErrorKind::ConfigInvalid)
                .with_detail("a request could not be built for the configured base URL")
        }
        ureq::Error::Protocol(_) | ureq::Error::LargeResponseHeader(_, _) => {
            ProviderError::new(ErrorKind::MalformedResponse)
                .with_detail("the reply was not valid HTTP")
        }
        _ => ProviderError::new(ErrorKind::Network).with_detail("the connection failed"),
    }
}

fn io_error_kind(kind: io::ErrorKind) -> ProviderError {
    match kind {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
            ProviderError::new(ErrorKind::TimedOut)
        }
        other => ProviderError::new(ErrorKind::Network).with_detail(format!("{other:?}")),
    }
}

/// A typed error for a failure while reading a response body. The HTTP client
/// wraps its own errors, a timeout among them, in an I/O error.
pub(crate) fn read_error(error: io::Error) -> ProviderError {
    transport_error(&ureq::Error::from(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(max_attempts: u32, initial_delay_ms: u64, max_delay_ms: u64) -> RetryPolicy {
        RetryPolicy {
            max_attempts,
            initial_delay_ms,
            max_delay_ms,
        }
    }

    #[test]
    fn statuses_map_to_kinds() {
        for (status, kind) in [
            (400, ErrorKind::RequestRejected),
            (401, ErrorKind::AuthenticationFailed),
            (402, ErrorKind::QuotaExhausted),
            (403, ErrorKind::AuthenticationFailed),
            (404, ErrorKind::RequestRejected),
            (408, ErrorKind::TimedOut),
            (413, ErrorKind::RequestRejected),
            (429, ErrorKind::RateLimited),
            (500, ErrorKind::Overloaded),
            (503, ErrorKind::Overloaded),
            (504, ErrorKind::TimedOut),
            (529, ErrorKind::Overloaded),
            (302, ErrorKind::RequestRejected),
        ] {
            assert_eq!(kind_for_status(status), kind, "{status}");
        }
    }

    #[test]
    fn retry_after_is_read_as_seconds() {
        assert_eq!(parse_retry_after("7"), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after(" 0 "), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("1.5"), Some(Duration::from_millis(1500)));
        assert_eq!(parse_retry_after("999999999"), Some(Duration::from_secs(86_400)));
        for unusable in ["", "soon", "-1", "NaN", "inf", "Wed, 21 Oct 2026 07:28:00 GMT"] {
            assert_eq!(parse_retry_after(unusable), None, "{unusable}");
        }
    }

    #[test]
    fn only_transient_errors_are_retried_and_only_up_to_the_limit() {
        let limited = ProviderError::new(ErrorKind::RateLimited);
        let rules = policy(3, 100, 1000);
        assert!(retry_delay(&rules, &limited, 1).is_some());
        assert!(retry_delay(&rules, &limited, 2).is_some());
        assert_eq!(retry_delay(&rules, &limited, 3), None);

        for kind in [
            ErrorKind::AuthenticationFailed,
            ErrorKind::RequestRejected,
            ErrorKind::Refusal,
            ErrorKind::MalformedResponse,
            ErrorKind::QuotaExhausted,
            ErrorKind::ConfigInvalid,
            ErrorKind::KeyUnavailable,
            ErrorKind::SpendLimitReached,
        ] {
            assert_eq!(retry_delay(&rules, &ProviderError::new(kind), 1), None, "{kind:?}");
        }
        for kind in [ErrorKind::Overloaded, ErrorKind::Network, ErrorKind::TimedOut] {
            assert!(retry_delay(&rules, &ProviderError::new(kind), 1).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn the_provider_delay_is_honoured_or_reported() {
        let rules = policy(3, 100, 1000);
        let short = ProviderError::new(ErrorKind::RateLimited)
            .with_retry_after(Some(Duration::from_millis(400)));
        assert_eq!(retry_delay(&rules, &short, 1), Some(Duration::from_millis(400)));

        let long = ProviderError::new(ErrorKind::RateLimited)
            .with_retry_after(Some(Duration::from_secs(60)));
        assert_eq!(retry_delay(&rules, &long, 1), None);
    }

    #[test]
    fn backoff_doubles_and_stays_under_the_ceiling() {
        let rules = policy(10, 100, 1000);
        for attempt in 1..=10 {
            let delay = backoff(&rules, attempt).as_millis();
            let base = (100u128 << (attempt - 1)).min(1000);
            assert!(delay >= base, "attempt {attempt}: {delay} < {base}");
            assert!(delay <= 1000, "attempt {attempt}: {delay}");
        }
        assert_eq!(backoff(&policy(3, 0, 1000), 2), Duration::ZERO);
        assert!(backoff(&policy(3, u64::MAX, 1000), 40) <= Duration::from_millis(1000));
    }

    #[test]
    fn client_errors_become_typed_errors_without_their_text() {
        let timeout = read_error(io::Error::new(io::ErrorKind::TimedOut, "secret detail"));
        assert_eq!(timeout.kind(), ErrorKind::TimedOut);
        assert!(!timeout.to_string().contains("secret detail"));

        let reset = read_error(io::Error::new(io::ErrorKind::ConnectionReset, "secret detail"));
        assert_eq!(reset.kind(), ErrorKind::Network);
        assert!(!reset.to_string().contains("secret detail"));

        let wrapped = read_error(ureq::Error::Timeout(ureq::Timeout::Global).into_io());
        assert_eq!(wrapped.kind(), ErrorKind::TimedOut);

        let bad_url = transport_error(&ureq::Error::BadUri("https://secret-host".to_owned()));
        assert_eq!(bad_url.kind(), ErrorKind::ConfigInvalid);
        assert!(!bad_url.to_string().contains("secret-host"));
    }
}
