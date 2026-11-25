//! This macOS-only module implements DNS resolution
//! using macOS's native system resolver. We have to
//! do this rather than relying on the default behavior
//! (which reads /etc/resolv.conf) since default behavior
//! breaks Codex's ability to interact with network paths
//! that are provided by applications that modify / hook
//! into macOS DNS behavior (i.e. proxies, VPNs, etc)
use rama_core::bytes::Bytes;
use rama_core::futures::Stream;
use rama_core::futures::async_stream::stream_fn;
use rama_core::futures::stream;
use rama_dns::client::resolver::DnsAddressResolver;
use rama_dns::client::resolver::DnsResolver;
use rama_dns::client::resolver::DnsTxtResolver;
use rama_net::address::Domain;
use std::io;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use tokio::net::lookup_host;

/// Resolve TCP destinations through macOS's native resolver, including supplemental
/// resolvers and VPN split DNS that are not represented in `/etc/resolv.conf`.
#[derive(Clone)]
pub(crate) struct SystemDnsResolver;

impl DnsAddressResolver for SystemDnsResolver {
    type Error = io::Error;

    fn lookup_ipv4(&self, domain: Domain) -> impl Stream<Item = io::Result<Ipv4Addr>> + Send + '_ {
        native_addresses(domain, |ip| match ip {
            IpAddr::V4(addr) => Some(addr),
            IpAddr::V6(_) => None,
        })
    }

    fn lookup_ipv6(&self, domain: Domain) -> impl Stream<Item = io::Result<Ipv6Addr>> + Send + '_ {
        native_addresses(domain, |ip| match ip {
            IpAddr::V6(addr) => Some(addr),
            IpAddr::V4(_) => None,
        })
    }
}

fn native_addresses<T: Send + 'static>(
    domain: Domain,
    select: fn(IpAddr) -> Option<T>,
) -> impl Stream<Item = io::Result<T>> + Send {
    stream_fn(async move |mut yielder| {
        match lookup_host((domain.as_str(), 0)).await {
            Ok(addrs) => {
                for addr in addrs.filter_map(|addr| select(addr.ip())) {
                    yielder.yield_item(Ok(addr)).await;
                }
            }
            Err(err) => {
                yielder.yield_item(Err(err)).await;
            }
        }
    })
}

impl DnsTxtResolver for SystemDnsResolver {
    type Error = io::Error;

    fn lookup_txt(&self, _domain: Domain) -> impl Stream<Item = io::Result<Bytes>> + Send + '_ {
        stream::once(std::future::ready(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the system address resolver does not support TXT lookups",
        ))))
    }
}

impl DnsResolver for SystemDnsResolver {}

#[cfg(test)]
#[path = "system_dns_tests.rs"]
mod tests;
