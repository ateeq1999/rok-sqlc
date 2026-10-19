// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! An in-process, per-IP rate limit: [`RateLimit`], an actix-web middleware
//! the `/auth` routes are wrapped in (`.wrap(limit.clone())`). Each client
//! IP may make `AUTH_RATE_LIMIT` requests a minute, a burst of that many
//! and then one every `60 / AUTH_RATE_LIMIT` seconds (the GCRA of the
//! `governor` crate); one more answers `429 too_many_requests` with
//! `Retry-After`. Every clone counts against the same limits, so build it
//! once (`AppState::new`) and share it between workers and routes.
//!
//! The IP is the connection's peer, or, with `TRUST_PROXY=true`, the last
//! address of `X-Forwarded-For`: the one your proxy appended. Trust it only
//! behind a proxy you run, since a client sets the header to anything it
//! likes. Limits live in this process: several instances each count their
//! own.

use std::future::{Future, Ready, ready};
use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU32;
use std::pin::Pin;
use std::sync::Arc;

use actix_web::ResponseError;
use actix_web::body::EitherBody;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready};
use actix_web::http::header::HeaderMap;
use governor::clock::Clock;
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::error::AppError;

/// Above this many tracked IPs, a check first forgets those whose limit
/// has fully recovered, so memory stays bounded.
const RETAIN_ABOVE: usize = 10_000;

/// The header a trusted proxy puts the client's address in.
const FORWARDED_FOR: &str = "x-forwarded-for";

/// A per-IP limit, shared by every clone.
#[derive(Clone)]
pub struct RateLimit {
    /// `None` when the limit is off (`AUTH_RATE_LIMIT=0`).
    limiter: Option<Arc<DefaultKeyedRateLimiter<IpAddr>>>,
    trust_proxy: bool,
}

impl RateLimit {
    /// `requests` a minute per IP (`0` turns the limit off), the IP taken
    /// from `X-Forwarded-For` when `trust_proxy`.
    #[must_use]
    pub fn per_minute(requests: u32, trust_proxy: bool) -> Self {
        let limiter = NonZeroU32::new(requests)
            .map(|requests| Arc::new(RateLimiter::keyed(Quota::per_minute(requests))));
        RateLimit {
            limiter,
            trust_proxy,
        }
    }

    /// Counts one request from `req`'s client: `Ok` while under the limit,
    /// else `TooManyRequests`.
    fn check(&self, req: &ServiceRequest) -> Result<(), AppError> {
        let Some(limiter) = &self.limiter else {
            return Ok(());
        };
        let peer = req.peer_addr().map(|addr| addr.ip());
        let ip = client_ip(req.headers(), peer, self.trust_proxy);
        if limiter.len() > RETAIN_ABOVE {
            limiter.retain_recent();
        }
        limiter.check_key(&ip).map_err(|not_until| {
            let wait = not_until.wait_time_from(limiter.clock().now());
            AppError::TooManyRequests {
                retry_after_secs: wait.as_secs().max(1),
            }
        })
    }
}

/// The client's IP: the last valid address of `X-Forwarded-For` when
/// `trust_proxy` (falling back to the peer), else the peer. A request with
/// neither (a test request, a Unix socket) counts as `0.0.0.0`.
#[must_use]
pub fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>, trust_proxy: bool) -> IpAddr {
    let forwarded = || {
        headers
            .get_all(FORWARDED_FOR)
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .filter_map(|address| address.trim().parse::<IpAddr>().ok())
            .last()
    };
    let forwarded = if trust_proxy { forwarded() } else { None };
    forwarded
        .or(peer)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

impl<S, B> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Transform = RateLimited<S>;
    type InitError = ();
    type Future = Ready<Result<RateLimited<S>, ()>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimited {
            service,
            limit: self.clone(),
        }))
    }
}

/// A service wrapped in a [`RateLimit`].
pub struct RateLimited<S> {
    service: S,
    limit: RateLimit,
}

impl<S, B> Service<ServiceRequest> for RateLimited<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, actix_web::Error>>>>;

    forward_ready!(service);

    /// Answers the wire standard's `429` itself, past the limit, rather
    /// than failing the call.
    fn call(&self, req: ServiceRequest) -> Self::Future {
        if let Err(refused) = self.limit.check(&req) {
            let response = req.into_response(refused.error_response());
            return Box::pin(async move { Ok(response.map_into_right_body()) });
        }
        let admitted = self.service.call(req);
        Box::pin(async move { Ok(admitted.await?.map_into_left_body()) })
    }
}

#[cfg(test)]
mod tests {
    use actix_web::http::header::{HeaderName, HeaderValue};

    use super::*;

    fn forwarded_for(values: &[&'static str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                HeaderName::from_static(FORWARDED_FOR),
                HeaderValue::from_static(value),
            );
        }
        headers
    }

    const PEER: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

    #[test]
    fn the_peer_is_the_client_unless_the_proxy_is_trusted() {
        let headers = forwarded_for(&["203.0.113.7"]);
        assert_eq!(client_ip(&headers, Some(PEER), false), PEER);
        assert_eq!(
            client_ip(&headers, Some(PEER), true),
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))
        );
    }

    #[test]
    fn a_trusted_proxy_s_address_is_the_last_one_forwarded() {
        let headers = forwarded_for(&["198.51.100.1, 203.0.113.7", "2001:db8::1"]);
        assert_eq!(
            client_ip(&headers, Some(PEER), true),
            IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1))
        );
    }

    #[test]
    fn without_a_usable_address_the_peer_or_unspecified_is_used() {
        let headers = forwarded_for(&["not an address"]);
        assert_eq!(client_ip(&headers, Some(PEER), true), PEER);
        assert_eq!(
            client_ip(&HeaderMap::new(), None, false),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }
}
