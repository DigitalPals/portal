use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => is_private_ipv6(v6),
    }
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

fn is_private_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
}

/// Decide whether an unencrypted-VNC warning must be shown before connecting.
///
/// - `resolved_addrs`: every address the target hostname resolved to; if any
///   of them is non-private the target is treated as public.
/// - `tunneled`: the connection is carried over an SSH tunnel (encrypted),
///   so no warning is needed.
/// - `allow_cleartext`: the user ticked "don't warn again for this host".
///
/// An empty address list (resolution failed) does not warn: the connection
/// attempt will fail with a clear DNS error instead.
pub fn should_warn_cleartext_vnc(
    resolved_addrs: &[IpAddr],
    tunneled: bool,
    allow_cleartext: bool,
) -> bool {
    if tunneled || allow_cleartext {
        return false;
    }
    resolved_addrs.iter().any(|addr| !is_private_ip(*addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public_v4() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10))
    }

    fn private_v4() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5))
    }

    fn loopback_v4() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    #[test]
    fn warns_for_public_target() {
        assert!(should_warn_cleartext_vnc(&[public_v4()], false, false));
    }

    #[test]
    fn warns_when_any_resolved_address_is_public() {
        assert!(should_warn_cleartext_vnc(
            &[private_v4(), public_v4()],
            false,
            false
        ));
    }

    #[test]
    fn does_not_warn_for_private_and_loopback_targets() {
        assert!(!should_warn_cleartext_vnc(&[private_v4()], false, false));
        assert!(!should_warn_cleartext_vnc(&[loopback_v4()], false, false));
        assert!(!should_warn_cleartext_vnc(
            &[private_v4(), loopback_v4()],
            false,
            false
        ));
    }

    #[test]
    fn does_not_warn_when_tunneled_over_ssh() {
        assert!(!should_warn_cleartext_vnc(&[public_v4()], true, false));
    }

    #[test]
    fn does_not_warn_when_host_allows_cleartext() {
        assert!(!should_warn_cleartext_vnc(&[public_v4()], false, true));
    }

    #[test]
    fn does_not_warn_when_resolution_yields_no_addresses() {
        assert!(!should_warn_cleartext_vnc(&[], false, false));
    }

    #[test]
    fn warns_for_public_ipv6_target() {
        let public_v6: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(should_warn_cleartext_vnc(&[public_v6], false, false));
        let ula_v6: IpAddr = "fd12:3456:789a::1".parse().unwrap();
        assert!(!should_warn_cleartext_vnc(&[ula_v6], false, false));
    }
}
