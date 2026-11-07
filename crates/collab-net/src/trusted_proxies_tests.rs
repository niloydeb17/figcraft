//! The reverse proxies a server believes: single addresses and ranges, IPv4 and IPv6.

use std::net::IpAddr;

use crate::TrustedProxies;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn addresses_and_ranges_are_believed_and_nothing_else() {
    let t = TrustedProxies::parse("127.0.0.1, 10.0.0.0/8 172.16.0.0/12,fd00::/8").unwrap();
    for yes in ["127.0.0.1", "10.0.0.1", "10.255.255.255", "172.16.0.9", "172.31.255.1", "fd12::1", "::ffff:10.1.2.3"] {
        assert!(t.contains(ip(yes)), "{yes}");
    }
    for no in ["127.0.0.2", "11.0.0.1", "172.32.0.1", "203.0.113.1", "fe80::1", "::1"] {
        assert!(!t.contains(ip(no)), "{no}");
    }
    assert!(TrustedProxies::parse("0.0.0.0/0").unwrap().contains(ip("203.0.113.1")), "everything, when asked for");
    assert!(TrustedProxies::single(ip("::ffff:127.0.0.1")).contains(ip("127.0.0.1")));
}

#[test]
fn nothing_set_believes_nobody_and_junk_is_refused() {
    let empty = TrustedProxies::parse("  ").unwrap();
    assert!(empty.is_empty() && !empty.contains(ip("127.0.0.1")));
    assert_eq!(TrustedProxies::default(), empty);
    for bad in ["localhost", "10.0.0.0/33", "fd00::/129", "10.0.0.0/", "10.0.0.0/x", "1.2.3.4/8/9"] {
        assert!(TrustedProxies::parse(bad).is_err(), "{bad}");
    }
    let nine = (1..=9).map(|i| format!("10.0.0.{i}")).collect::<Vec<_>>().join(",");
    assert!(TrustedProxies::parse(&nine).is_err(), "at most {}", TrustedProxies::MAX);
}
