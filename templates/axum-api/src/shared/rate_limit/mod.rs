// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! An in-process, per-IP rate limit: [`RateLimit`], a tower `Layer` the
//! `/auth` routes are wrapped in (`.layer(limit.clone())`). Each client IP
//! may make `AUTH_RATE_LIMIT` requests a minute, a burst of that many and
//! then one every `60 / AUTH_RATE_LIMIT` seconds (the GCRA of the
//! `governor` crate); one more answers `429 too_many_requests` with
//! `Retry-After`. Every clone counts against the same limits, so build it
//! once (`AppState::new`) and share it between routes.
//!
//! The IP is the connection's peer (`ConnectInfo`, which `app::serve`
//! provides), or, with `TRUST_PROXY=true`, the last address of
//! `X-Forwarded-For`: the one your proxy appended. Trust it only behind a
//! proxy you run, since a client sets the header to anything it likes.
//! Limits live in this process: several instances each count their own.
//!
//! The counting is [`limiter`], the same in both API templates; this file
//! is axum's middleware around it.

mod limiter;

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Once;
use std::task::{Context, Poll};

use axum::extract::{ConnectInfo, Request};
use axum::response::{IntoResponse, Response};
use tower::{Layer, Service};

pub use self::limiter::{FORWARDED_FOR, RateLimit, client_ip};

/// Done once a limited request came without the peer's address.
static MISSING_PEER: Once = Once::new();

/// The connection's peer, from the `ConnectInfo` that `app::serve`'s
/// `into_make_service_with_connect_info` provides. Without it every client
/// counts as `0.0.0.0`, sharing one limit, so the first such request logs
/// a warning.
fn peer(req: &Request) -> Option<IpAddr> {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    if peer.is_none() {
        MISSING_PEER.call_once(|| {
            tracing::warn!(
                "a rate-limited request has no peer address (`ConnectInfo`): every client \
                 shares one limit; serve the router with \
                 `into_make_service_with_connect_info::<SocketAddr>()`"
            );
        });
    }
    peer
}

impl<S> Layer<S> for RateLimit {
    type Service = RateLimited<S>;

    fn layer(&self, inner: S) -> RateLimited<S> {
        RateLimited {
            inner,
            limit: self.clone(),
        }
    }
}

/// A service wrapped in a [`RateLimit`].
#[derive(Clone)]
pub struct RateLimited<S> {
    inner: S,
    limit: RateLimit,
}

impl<S> Service<Request> for RateLimited<S>
where
    S: Service<Request, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    /// Answers the wire standard's `429` itself, past the limit, without
    /// calling the inner service.
    fn call(&mut self, req: Request) -> Self::Future {
        let forwarded_for = req
            .headers()
            .get_all(FORWARDED_FOR)
            .iter()
            .filter_map(|value| value.to_str().ok());
        if let Err(refused) = self.limit.check(forwarded_for, || peer(&req)) {
            return Box::pin(async move { Ok(refused.into_response()) });
        }
        Box::pin(self.inner.call(req))
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use axum::body::Body;

    use super::*;

    #[test]
    fn a_request_without_connect_info_has_no_peer_and_warns() {
        let request = Request::new(Body::empty());
        assert_eq!(peer(&request), None);
        assert!(MISSING_PEER.is_completed());
    }

    #[test]
    fn a_request_with_connect_info_has_its_peer() {
        let mut request = Request::new(Body::empty());
        let addr = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 5000));
        request.extensions_mut().insert(ConnectInfo(addr));
        assert_eq!(peer(&request), Some(addr.ip()));
    }
}
