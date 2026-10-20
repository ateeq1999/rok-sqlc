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
//!
//! The counting is [`limiter`], the same in both API templates; this file
//! is actix-web's middleware around it.

mod limiter;

use std::future::{Future, Ready, ready};
use std::pin::Pin;

use actix_web::ResponseError;
use actix_web::body::EitherBody;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready};

pub use self::limiter::{FORWARDED_FOR, RateLimit, client_ip};

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
        let forwarded_for = req
            .headers()
            .get_all(FORWARDED_FOR)
            .filter_map(|value| value.to_str().ok());
        let checked = self
            .limit
            .check(forwarded_for, || req.peer_addr().map(|addr| addr.ip()));
        if let Err(refused) = checked {
            let response = req.into_response(refused.error_response());
            return Box::pin(async move { Ok(response.map_into_right_body()) });
        }
        let admitted = self.service.call(req);
        Box::pin(async move { Ok(admitted.await?.map_into_left_body()) })
    }
}
