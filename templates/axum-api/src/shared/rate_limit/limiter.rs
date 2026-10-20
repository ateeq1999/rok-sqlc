// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The rate limit's framework-free core: [`RateLimit`], which counts each
//! client IP's requests (the GCRA of the `governor` crate), and
//! [`client_ip`], which picks the IP. `rate_limit/mod.rs` wraps it in the
//! web framework's middleware. Both API templates hold the same copy of
//! this file.

use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU32;
use std::sync::Arc;

use governor::clock::Clock;
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::error::AppError;

/// Above this many tracked IPs, a check first forgets those whose limit
/// has fully recovered, so memory stays bounded.
const RETAIN_ABOVE: usize = 10_000;

/// The header a trusted proxy puts the client's address in.
pub const FORWARDED_FOR: &str = "x-forwarded-for";

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

    /// Counts one request from a client (see [`client_ip`]): its
    /// `X-Forwarded-For` values `forwarded_for`, and `peer`, asked only
    /// while the limit is on. `Ok` while under the limit, else
    /// `TooManyRequests`.
    pub fn check<'a>(
        &self,
        forwarded_for: impl IntoIterator<Item = &'a str>,
        peer: impl FnOnce() -> Option<IpAddr>,
    ) -> Result<(), AppError> {
        let Some(limiter) = &self.limiter else {
            return Ok(());
        };
        let ip = client_ip(forwarded_for, peer(), self.trust_proxy);
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

/// The client's IP: the last valid address of `X-Forwarded-For`
/// (`forwarded_for`, its header values) when `trust_proxy`, falling back
/// to the peer; else the peer. A request with neither (a test request, a
/// Unix socket, a server without the peer's address) counts as `0.0.0.0`.
#[must_use]
pub fn client_ip<'a>(
    forwarded_for: impl IntoIterator<Item = &'a str>,
    peer: Option<IpAddr>,
    trust_proxy: bool,
) -> IpAddr {
    let forwarded = if trust_proxy {
        forwarded_for
            .into_iter()
            .flat_map(|value| value.split(','))
            .filter_map(|address| address.trim().parse::<IpAddr>().ok())
            .last()
    } else {
        None
    };
    forwarded
        .or(peer)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

    #[test]
    fn the_peer_is_the_client_unless_the_proxy_is_trusted() {
        let forwarded = ["203.0.113.7"];
        assert_eq!(client_ip(forwarded, Some(PEER), false), PEER);
        assert_eq!(
            client_ip(forwarded, Some(PEER), true),
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))
        );
    }

    #[test]
    fn a_trusted_proxy_s_address_is_the_last_one_forwarded() {
        let forwarded = ["198.51.100.1, 203.0.113.7", "2001:db8::1"];
        assert_eq!(
            client_ip(forwarded, Some(PEER), true),
            IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1))
        );
    }

    #[test]
    fn without_a_usable_address_the_peer_or_unspecified_is_used() {
        assert_eq!(client_ip(["not an address"], Some(PEER), true), PEER);
        assert_eq!(
            client_ip([], None, false),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }

    #[test]
    fn past_the_limit_a_client_is_refused_until_it_recovers() {
        let limit = RateLimit::per_minute(1, false);
        assert!(limit.check([], || Some(PEER)).is_ok());
        let refused = limit.check([], || Some(PEER));
        assert!(
            matches!(refused, Err(AppError::TooManyRequests { retry_after_secs }) if retry_after_secs >= 1),
            "{refused:?}"
        );
        assert!(limit.check([], || None).is_ok(), "another client");
    }

    #[test]
    fn an_unlimited_check_never_asks_for_the_peer() {
        let limit = RateLimit::per_minute(0, false);
        for _ in 0..5 {
            assert!(
                limit
                    .check([], || unreachable!("the peer is asked"))
                    .is_ok()
            );
        }
    }
}
