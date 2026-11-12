// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The HTTP front: hyper's HTTP/1 server on a single-threaded tokio
//! runtime, behind a reverse proxy that terminates TLS (Caddy, see
//! `deploy/Caddyfile`).
//!
//! * `POST /v1/reports`: rate limits are counted first, then the media type
//!   and the declared length are checked, then the body is read with a size
//!   and a time limit, and only then is it verified, on a blocking thread,
//!   with at most `max_concurrent_uploads` at once.
//! * `GET /v1/reports/<token>`: the state of one report.
//! * Anything else: `TUR-DIAG-RECEIVER-001` or `-002`.
//!
//! The client's address is the peer's, unless the peer is a trusted proxy,
//! in which case it is the last address of `X-Forwarded-For` (the one the
//! proxy added). It is used for the rate limit and nothing else.
//!
//! Filing and the sweep run on two plain threads beside the server.

use std::convert::Infallible;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Semaphore;

use crate::codes::Refusal;
use crate::receiver::{Answer, Receiver};
use crate::tracker::Tracker;

/// The path of uploads.
pub const UPLOAD_PATH: &str = tur_diag::report::UPLOAD_PATH;
/// The prefix of status addresses.
pub const STATUS_PREFIX: &str = "/v1/reports/";

struct Shared {
    receiver: Arc<Receiver>,
    uploads: Arc<Semaphore>,
}

/// The client's address: the peer's, or, when the peer is a trusted proxy,
/// the last entry of `X-Forwarded-For`.
pub fn client_address(peer: IpAddr, forwarded: Option<&str>, trusted: &[IpAddr]) -> IpAddr {
    let peer = peer.to_canonical();
    if !trusted.contains(&peer) {
        return peer;
    }
    forwarded
        .and_then(|value| value.rsplit(',').next())
        .and_then(|last| last.trim().parse::<IpAddr>().ok())
        .map_or(peer, |address| address.to_canonical())
}

/// Turns an answer into an HTTP response.
pub fn respond(answer: Answer) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(answer.body)));
    *response.status_mut() =
        StatusCode::from_u16(answer.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    if let Some(after) = answer.retry_after
        && let Ok(value) = HeaderValue::from_str(&after.to_string())
    {
        headers.insert(RETRY_AFTER, value);
    }
    response
}

async fn blocking<F>(receiver: &Arc<Receiver>, work: F) -> Answer
where
    F: FnOnce() -> Answer + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(answer) => answer,
        Err(_) => receiver.refuse(Refusal::Internal, None),
    }
}

fn media_type(value: Option<&str>) -> Option<String> {
    value
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().to_ascii_lowercase())
}

/// Reads and drops a small body, so that a refusal sent before the body
/// was read is not cut off by a connection reset. A large body is not read:
/// clients that send one wait for `100 Continue` first.
async fn discard(body: Incoming) {
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        Limited::new(body, 64 * 1024).collect(),
    )
    .await;
}

async fn upload(request: Request<Incoming>, client: IpAddr, shared: &Shared) -> Answer {
    let receiver = &shared.receiver;
    let (parts, body) = request.into_parts();
    if let Some(refusal) = receiver.admit_upload(client) {
        discard(body).await;
        return refusal;
    }
    let content_type = parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if media_type(content_type.as_deref()).as_deref() != Some(tur_diag::report::CONTENT_TYPE) {
        discard(body).await;
        return receiver.refuse(Refusal::UnsupportedMediaType, None);
    }
    let limits = &receiver.config().limits;
    let declared = parts
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared.is_some_and(|length| length > limits.max_upload_bytes) {
        return receiver.refuse(Refusal::TooLarge, None);
    }
    let Ok(permit) = Arc::clone(&shared.uploads).try_acquire_owned() else {
        discard(body).await;
        return receiver.refuse(Refusal::Busy, None);
    };
    let max = usize::try_from(limits.max_upload_bytes).unwrap_or(usize::MAX);
    let timeout = Duration::from_secs(receiver.config().timing.upload_timeout_seconds);
    let collected = tokio::time::timeout(timeout, Limited::new(body, max).collect()).await;
    let body = match collected {
        Err(_) => return receiver.refuse(Refusal::Incomplete, None),
        Ok(Err(error)) => {
            let refusal = if error.downcast_ref::<LengthLimitError>().is_some() {
                Refusal::TooLarge
            } else {
                Refusal::Incomplete
            };
            return receiver.refuse(refusal, None);
        }
        Ok(Ok(collected)) => collected.to_bytes(),
    };
    let worker = Arc::clone(receiver);
    let answer = blocking(receiver, move || {
        worker.submit(content_type.as_deref(), &body)
    })
    .await;
    drop(permit);
    answer
}

async fn handle(
    request: Request<Incoming>,
    peer: IpAddr,
    shared: Arc<Shared>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let receiver = Arc::clone(&shared.receiver);
    let forwarded = request
        .headers()
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let client = client_address(peer, forwarded.as_deref(), &receiver.config().proxy.trusted);
    let path = request.uri().path().to_owned();
    let answer = if path == UPLOAD_PATH {
        if request.method() == Method::POST {
            upload(request, client, &shared).await
        } else {
            receiver.refuse(Refusal::MethodNotAllowed, None)
        }
    } else if let Some(token) = path.strip_prefix(STATUS_PREFIX) {
        if request.method() == Method::GET {
            let token = token.to_owned();
            let worker = Arc::clone(&receiver);
            blocking(&receiver, move || worker.status(client, &token)).await
        } else {
            receiver.refuse(Refusal::MethodNotAllowed, None)
        }
    } else {
        receiver.refuse(Refusal::NotFound, None)
    };
    Ok(respond(answer))
}

/// Sleeps for `seconds`, waking early when `stop` is set.
fn pause(stop: &AtomicBool, seconds: u64) {
    for _ in 0..seconds.saturating_mul(10) {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Starts filing and the sweep on threads of their own. They run until
/// `stop` is set.
pub fn spawn_background(
    receiver: Arc<Receiver>,
    tracker: Arc<dyn Tracker>,
    stop: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>> {
    let filer = {
        let receiver = Arc::clone(&receiver);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                receiver.file_due(tracker.as_ref());
                pause(&stop, receiver.config().timing.file_interval_seconds);
            }
        })
    };
    let sweeper = thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            if receiver.sweep().is_err() {
                receiver.log().warn("sweep-failed", &[]);
            }
            pause(&stop, receiver.config().timing.sweep_interval_seconds);
        }
    });
    vec![filer, sweeper]
}

async fn accept_loop(receiver: Arc<Receiver>) -> Result<(), String> {
    let address = receiver.config().listen;
    let listener = TcpListener::bind(address)
        .await
        .map_err(|error| format!("{address}: {error}"))?;
    let mut terminate =
        signal(SignalKind::terminate()).map_err(|error| format!("signals: {error}"))?;
    let shown = address.to_string();
    receiver
        .log()
        .info("listening", &[("address", shown.as_str())]);
    let shared = Arc::new(Shared {
        uploads: Arc::new(Semaphore::new(
            receiver.config().limits.max_concurrent_uploads,
        )),
        receiver: Arc::clone(&receiver),
    });
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else {
                    continue;
                };
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        handle(request, peer.ip(), Arc::clone(&shared))
                    });
                    // A connection that fails is the client's problem; it
                    // has already been answered or has gone away.
                    let _ = http1::Builder::new()
                        .timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(20))
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
            _ = terminate.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    receiver.log().info("stopping", &[]);
    Ok(())
}

/// Runs the receiver until it is told to stop (SIGTERM or SIGINT).
pub fn serve(receiver: Arc<Receiver>, tracker: Arc<dyn Tracker>) -> Result<(), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let workers = spawn_background(Arc::clone(&receiver), tracker, Arc::clone(&stop));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("runtime: {error}"))?;
    let result = runtime.block_on(accept_loop(receiver));
    stop.store(true, Ordering::SeqCst);
    for worker in workers {
        let _ = worker.join();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_trusted_proxy_may_name_the_client() {
        let proxy: IpAddr = "127.0.0.1".parse().unwrap();
        let stranger: IpAddr = "198.51.100.9".parse().unwrap();
        let trusted = [proxy];
        assert_eq!(
            client_address(proxy, Some("10.0.0.1, 203.0.113.5"), &trusted),
            "203.0.113.5".parse::<IpAddr>().unwrap()
        );
        assert_eq!(client_address(proxy, None, &trusted), proxy);
        assert_eq!(client_address(proxy, Some("garbage"), &trusted), proxy);
        assert_eq!(
            client_address(stranger, Some("203.0.113.5"), &trusted),
            stranger
        );
        let mapped: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert_eq!(
            client_address(mapped, Some("203.0.113.6"), &trusted),
            "203.0.113.6".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn answers_are_json_and_never_cached() {
        let response = respond(Answer {
            status: 429,
            body: "{}".to_owned(),
            retry_after: Some(60),
        });
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers();
        assert_eq!(headers[CONTENT_TYPE], "application/json");
        assert_eq!(headers[CACHE_CONTROL], "no-store");
        assert_eq!(headers[RETRY_AFTER], "60");
        assert_eq!(
            media_type(Some("Application/Vnd.Tur.Diag-Bundle+Zstd; x=1")).as_deref(),
            Some("application/vnd.tur.diag-bundle+zstd")
        );
    }
}
