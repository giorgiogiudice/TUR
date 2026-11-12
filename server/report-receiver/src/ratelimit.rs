// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Rate limits: per address and for everyone, in fixed windows of an hour.
//!
//! An IPv6 client is counted by its /64 network, since one host usually
//! holds a whole /64; an IPv4-mapped IPv6 address counts as the IPv4
//! address. Addresses are held in memory only, for at most one window, and
//! are never written anywhere.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

/// The length of a window, in seconds.
pub const WINDOW_SECONDS: u64 = 3600;

/// How many addresses are kept before expired windows are dropped.
const PRUNE_AT: usize = 4096;

/// What a limiter decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The request may go ahead; it has been counted.
    Allowed,
    /// This address has used its allowance; it may try again after the
    /// given number of seconds.
    AddressLimited(u64),
    /// Everyone together has used the allowance.
    GlobalLimited(u64),
}

#[derive(Debug, Clone, Copy)]
struct Window {
    start: u64,
    count: u32,
}

impl Window {
    fn current(&mut self, now: u64) -> &mut Self {
        if now >= self.start.saturating_add(WINDOW_SECONDS) || now < self.start {
            self.start = now;
            self.count = 0;
        }
        self
    }

    fn retry_after(&self, now: u64) -> u64 {
        self.start
            .saturating_add(WINDOW_SECONDS)
            .saturating_sub(now)
            .max(1)
    }
}

#[derive(Debug)]
struct Inner {
    global: Window,
    addresses: HashMap<IpAddr, Window>,
}

/// A limiter for one kind of request.
#[derive(Debug)]
pub struct RateLimiter {
    per_address: u32,
    global: Option<u32>,
    inner: Mutex<Inner>,
}

/// The key an address is counted under.
pub fn address_key(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let network = u128::from(v6) & !u128::from(u64::MAX);
                IpAddr::V6(network.into())
            }
        },
    }
}

impl RateLimiter {
    /// A limiter allowing `per_address` requests per address and, if given,
    /// `global` requests in all per window.
    pub fn new(per_address: u32, global: Option<u32>) -> Self {
        RateLimiter {
            per_address,
            global,
            inner: Mutex::new(Inner {
                global: Window { start: 0, count: 0 },
                addresses: HashMap::new(),
            }),
        }
    }

    /// Counts a request from `address` at `now`, if it is allowed.
    pub fn check(&self, address: IpAddr, now: u64) -> Verdict {
        let Ok(mut inner) = self.inner.lock() else {
            // A poisoned lock means a thread panicked while counting. Refuse
            // rather than allow without limit.
            return Verdict::GlobalLimited(60);
        };
        if inner.addresses.len() >= PRUNE_AT {
            inner.addresses.retain(|_, window| {
                now < window.start.saturating_add(WINDOW_SECONDS) && now >= window.start
            });
        }
        let key = address_key(address);
        let mut window = *inner
            .addresses
            .entry(key)
            .or_insert(Window {
                start: now,
                count: 0,
            })
            .current(now);
        if window.count >= self.per_address {
            return Verdict::AddressLimited(window.retry_after(now));
        }
        if let Some(global) = self.global {
            let global_window = inner.global.current(now);
            if global_window.count >= global {
                return Verdict::GlobalLimited(global_window.retry_after(now));
            }
            global_window.count += 1;
        }
        window.count += 1;
        inner.addresses.insert(key, window);
        Verdict::Allowed
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn v4(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, last))
    }

    #[test]
    fn each_address_has_its_allowance_and_everyone_shares_one() {
        let limiter = RateLimiter::new(2, Some(3));
        assert_eq!(limiter.check(v4(1), 100), Verdict::Allowed);
        assert_eq!(limiter.check(v4(1), 101), Verdict::Allowed);
        assert_eq!(limiter.check(v4(1), 102), Verdict::AddressLimited(3598));
        assert_eq!(limiter.check(v4(2), 103), Verdict::Allowed);
        assert!(matches!(
            limiter.check(v4(3), 104),
            Verdict::GlobalLimited(_)
        ));
        // A refused request is not counted.
        assert!(matches!(
            limiter.check(v4(2), 105),
            Verdict::GlobalLimited(_)
        ));
        // A new window starts after an hour.
        assert_eq!(limiter.check(v4(1), 100 + WINDOW_SECONDS), Verdict::Allowed);
        assert_eq!(limiter.check(v4(3), 100 + WINDOW_SECONDS), Verdict::Allowed);
    }

    #[test]
    fn an_ipv6_network_counts_as_one_client() {
        let limiter = RateLimiter::new(1, None);
        let a: IpAddr = "2001:db8:1:2::1".parse::<Ipv6Addr>().unwrap().into();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse::<Ipv6Addr>().unwrap().into();
        let other: IpAddr = "2001:db8:1:3::1".parse::<Ipv6Addr>().unwrap().into();
        assert_eq!(limiter.check(a, 0), Verdict::Allowed);
        assert!(matches!(limiter.check(b, 1), Verdict::AddressLimited(_)));
        assert_eq!(limiter.check(other, 2), Verdict::Allowed);
        let mapped: IpAddr = "::ffff:203.0.113.7".parse::<Ipv6Addr>().unwrap().into();
        assert_eq!(address_key(mapped), v4(7));
    }

    #[test]
    fn old_windows_are_dropped_when_many_addresses_are_held() {
        let limiter = RateLimiter::new(1, None);
        for index in 0..PRUNE_AT {
            let address = IpAddr::V4(Ipv4Addr::from(u32::try_from(index).unwrap()));
            assert_eq!(limiter.check(address, 0), Verdict::Allowed);
        }
        assert_eq!(limiter.check(v4(1), WINDOW_SECONDS + 1), Verdict::Allowed);
        assert_eq!(limiter.inner.lock().unwrap().addresses.len(), 1);
    }
}
