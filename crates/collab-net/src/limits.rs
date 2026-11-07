//! Transport limits, shared by the native transport and the browser client.

use std::net::IpAddr;
use std::time::Duration;

/// Transport limits (the collaboration core has its own: [`figcraft_collab::Limits`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetLimits {
    /// Sockets a host keeps open at once, joined or not.
    pub max_connections: usize,
    /// Largest message a client may send once it has joined (a batch can carry images), and
    /// before: its hello.
    pub max_client_message: usize,
    pub max_hello: usize,
    /// Largest message a client accepts from the host (a welcome carries the whole document): 1 GiB,
    /// room for any file up to 1023 MiB and well past the servers' defaults, while a hostile server
    /// (any link can name one) can't make the app or a browser tab buffer more than that.
    pub max_server_message: usize,
    /// Messages a client may send per second on average, and in a burst; beyond that it is
    /// disconnected.
    pub rate: f64,
    pub burst: f64,
    /// A new connection must finish the WebSocket handshake and say hello within this.
    pub hello_timeout: Duration,
    /// A connection with no traffic for this long is dead.
    pub idle_timeout: Duration,
    /// How long a one-shot request connection (hosting, a library, a plugin directory — and, most
    /// importantly, publishing a file) waits for the server's reply before giving up. Publishing
    /// uploads a whole file and a large one on a slow link takes the server a while to receive and
    /// answer, so this is longer than `idle_timeout` (a large project on a slow connection can
    /// easily need several minutes).
    pub request_reply_timeout: Duration,
    /// A client pings this often, whatever else it sends (keeps it under `idle_timeout`).
    pub keepalive: Duration,
    /// Messages queued for one client before it counts as stuck and is disconnected.
    pub queue: usize,
    /// How long a write may block.
    pub write_timeout: Duration,
    /// Most connections from one address at once (an IPv6 address counts by its /64; 0: no cap).
    pub max_per_address: usize,
    /// The reverse proxy in front of a host, whose `X-Forwarded-For` names the client: only
    /// connections from these addresses are believed (the header is anyone's to forge).
    pub trusted_proxy: TrustedProxies,
    /// Bytes queued for one client before it counts as stuck (one message always fits an empty
    /// queue), for one block of addresses (an IPv4 address, an IPv6 /48), and for every client
    /// together (a broadcast counted once; past it the biggest consumer is dropped).
    pub max_queued_bytes: usize,
    pub max_queued_block: usize,
    pub max_queued_total: usize,
    /// Bytes being read into messages (not whole yet) for one block, and for every connection:
    /// past them the biggest readers are dropped.
    pub max_reading_block: usize,
    pub max_reading_total: usize,
    /// Bytes of messages handed to the hub and not handled yet, all connections together: past it
    /// a connection is dropped as busy instead of piling more on.
    pub max_in_flight: usize,
    /// Largest message on a connection that only asks small things (browsing a plugin directory).
    pub max_request: usize,
    /// Connections only sessions (`Hello`) and the operator may take: one-shot requests leave them.
    pub reserve_for_sessions: usize,
    /// A one-shot request connection (hosting, a library, a plugin directory) with no traffic for
    /// this long is dropped.
    pub request_idle: Duration,
}

impl Default for NetLimits {
    fn default() -> Self {
        Self {
            max_connections: 64,
            max_client_message: figcraft_collab::Limits::default().max_message_bytes,
            max_hello: 64 << 10,
            max_server_message: 1 << 30,
            rate: 100.0,
            burst: 400.0,
            hello_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(45),
            request_reply_timeout: Duration::from_secs(300),
            keepalive: Duration::from_secs(15),
            queue: 1024,
            write_timeout: Duration::from_secs(10),
            max_per_address: 0,
            trusted_proxy: TrustedProxies::default(),
            max_queued_bytes: 160 << 20,
            max_queued_block: usize::MAX,
            max_queued_total: usize::MAX,
            max_reading_block: usize::MAX,
            max_reading_total: usize::MAX,
            max_in_flight: usize::MAX,
            max_request: 64 << 10,
            reserve_for_sessions: 0,
            request_idle: Duration::from_secs(45),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_reply_wait_is_longer_than_idle() {
        let d = NetLimits::default();
        // Publishing uploads a whole file; a slow link must not cut it off at the session idle
        // timeout. Keep the reply wait comfortably above it.
        assert!(d.request_reply_timeout > d.idle_timeout);
    }
}

/// The addresses a reverse proxy may connect from: single addresses (`127.0.0.1`) or ranges
/// (`10.0.0.0/8`, `fd00::/8`), for a proxy whose own address isn't fixed (a container's); at most
/// [`TrustedProxies::MAX`]. Empty: no proxy is believed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrustedProxies {
    nets: [Option<(IpAddr, u8)>; TrustedProxies::MAX],
}

impl TrustedProxies {
    /// Most addresses and ranges kept.
    pub const MAX: usize = 8;

    /// Addresses and ranges separated by commas or spaces; why not, for anything else.
    pub fn parse(text: &str) -> Result<TrustedProxies, String> {
        let mut out = TrustedProxies::default();
        for (n, item) in text.split([',', ' ', '\t']).map(str::trim).filter(|s| !s.is_empty()).enumerate() {
            let (addr, bits) = match item.split_once('/') {
                Some((a, b)) => (a, Some(b)),
                None => (item, None),
            };
            let bad = || format!("`{item}` isn't an IP address or a range like 10.0.0.0/8");
            let ip: IpAddr = addr.parse().map_err(|_| bad())?;
            let ip = ip.to_canonical();
            let max = if ip.is_ipv4() { 32 } else { 128 };
            let bits = match bits {
                Some(b) => b.parse::<u8>().ok().filter(|b| *b <= max).ok_or_else(bad)?,
                None => max,
            };
            let slot = out.nets.get_mut(n).ok_or_else(|| format!("at most {} addresses or ranges", TrustedProxies::MAX))?;
            *slot = Some((ip, bits));
        }
        Ok(out)
    }

    /// One address.
    pub fn single(ip: IpAddr) -> TrustedProxies {
        let ip = ip.to_canonical();
        let mut out = TrustedProxies::default();
        if let Some(first) = out.nets.first_mut() {
            *first = Some((ip, if ip.is_ipv4() { 32 } else { 128 }));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.nets.iter().all(Option::is_none)
    }

    /// `ip` is one of them (an IPv4 address written as IPv6 counts as itself).
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.nets.iter().flatten().any(|&(net, bits)| match (net, ip) {
            (IpAddr::V4(n), IpAddr::V4(a)) => prefix(u128::from(u32::from(n)) << 96, u128::from(u32::from(a)) << 96, bits),
            (IpAddr::V6(n), IpAddr::V6(a)) => prefix(u128::from(n), u128::from(a), bits),
            _ => false,
        })
    }
}

/// `a` and `b` (left-aligned in 128 bits) agree in their first `bits` bits.
fn prefix(a: u128, b: u128, bits: u8) -> bool {
    match bits {
        0 => true,
        1..=127 => (a ^ b) >> (128 - u32::from(bits)) == 0,
        _ => a == b,
    }
}
