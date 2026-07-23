//! Host port forwarding for the managed VM.
//!
//! This is the single biggest thing standing between Hopper's own engine and
//! being usable. `hopperd` gave the guest NAT networking and bridged only the
//! Docker socket, so `docker run -p 8080:80 nginx` bound port 8080 *inside the
//! VM* and `http://localhost:8080` on the Mac hit nothing. Every tutorial,
//! README, and compose file in existence assumes otherwise.
//!
//! The design mirrors what Docker Desktop, OrbStack, and Colima do: watch the
//! daemon for published ports, keep one host listener per mapping, and splice
//! each accepted connection through vsock to a forwarding agent in the guest.
//!
//! The diffing and the mapping-extraction are pure, so the part that decides
//! *what* to listen on is tested without a VM.

use model::Container;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

/// The guest vsock port where the forwarding agent listens.
pub const AGENT_VSOCK_PORT: u32 = 2378;

/// One host→guest mapping.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Forward {
    /// The address to bind on the host.
    pub host_ip: IpAddr,
    pub host_port: u16,
    /// The port to connect to *inside the guest*.
    ///
    /// This is the **published** port, not the container-internal one. Docker
    /// runs inside the guest, so from its point of view the guest is the host:
    /// `-p 18080:80` binds 18080 on the guest and 80 only inside the
    /// container's namespace. Connecting to 80 would reach nothing.
    pub guest_port: u16,
    pub proto: String,
}

impl Forward {
    /// The line the guest agent reads to know where to connect.
    pub fn agent_request(&self) -> String {
        format!("{}:{}\n", guest_target(&self.proto), self.guest_port)
    }

    pub fn bind_addr(&self) -> (IpAddr, u16) {
        (self.host_ip, self.host_port)
    }
}

/// Inside the guest, published ports are bound on all interfaces, so the agent
/// connects over loopback.
fn guest_target(_proto: &str) -> &'static str {
    "127.0.0.1"
}

/// Normalize the address Docker reports for a published port.
///
/// Docker reports `0.0.0.0` for "all interfaces". Binding that on the host
/// would expose the container to the local network, which is a meaningful
/// security change from what the user asked for and is *not* what Docker
/// Desktop does — it binds loopback. An explicit non-wildcard address is
/// honored as written.
pub fn normalize_bind(ip: Option<&str>) -> IpAddr {
    match ip.map(str::trim).filter(|s| !s.is_empty()) {
        None => IpAddr::V4(Ipv4Addr::LOCALHOST),
        Some("0.0.0.0") | Some("::") => IpAddr::V4(Ipv4Addr::LOCALHOST),
        Some(addr) => addr
            .parse::<IpAddr>()
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    }
}

/// Every mapping the running containers ask for.
///
/// Only running containers count: a stopped container's ports are not bound
/// inside the guest either, and holding the host port would block the next
/// thing that wants it.
pub fn wanted_forwards(containers: &[Container]) -> BTreeSet<Forward> {
    containers
        .iter()
        .filter(|c| c.state == model::ContainerState::Running)
        .flat_map(|c| c.ports.iter())
        .filter_map(|p| {
            let host_port = p.public_port?;
            // UDP needs a datagram path rather than a stream splice; forwarding
            // it over the TCP-shaped agent would silently corrupt traffic, so
            // it is left out until the agent grows a datagram mode.
            if !p.proto.eq_ignore_ascii_case("tcp") {
                return None;
            }
            Some(Forward {
                host_ip: normalize_bind(p.ip.as_deref()),
                host_port,
                guest_port: host_port,
                proto: p.proto.to_lowercase(),
            })
        })
        .collect()
}

/// What to change to move from `current` to `wanted`.
pub fn diff(
    current: &BTreeSet<Forward>,
    wanted: &BTreeSet<Forward>,
) -> (Vec<Forward>, Vec<Forward>) {
    let start = wanted.difference(current).cloned().collect();
    let stop = current.difference(wanted).cloned().collect();
    (start, stop)
}

/// Why a listener could not be opened, phrased for a human.
pub fn bind_failure(f: &Forward, err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::AddrInUse => format!(
            "Port {} is already in use on your Mac, so it could not be forwarded. \
             Stop whatever is using it, or publish the container on a different port.",
            f.host_port
        ),
        std::io::ErrorKind::PermissionDenied => format!(
            "Port {} needs elevated privileges to bind. Ports below 1024 are \
             restricted; publish the container on a higher port instead.",
            f.host_port
        ),
        _ => format!("Could not forward port {}: {err}", f.host_port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ContainerState, Health, Port};
    use std::collections::BTreeMap;

    fn container(state: ContainerState, ports: Vec<Port>) -> Container {
        Container {
            id: "id".into(),
            name: "c".into(),
            image: "img".into(),
            image_id: String::new(),
            command: String::new(),
            created: 0,
            state,
            status: String::new(),
            health: Health::None,
            ports,
            labels: BTreeMap::new(),
            mounts: vec![],
            networks: vec![],
            compose_project: None,
            compose_service: None,
        }
    }

    fn port(ip: Option<&str>, public: Option<u16>, private: u16, proto: &str) -> Port {
        Port {
            ip: ip.map(str::to_string),
            private_port: private,
            public_port: public,
            proto: proto.into(),
        }
    }

    #[test]
    fn a_published_port_becomes_a_forward() {
        let list = vec![container(
            ContainerState::Running,
            vec![port(Some("0.0.0.0"), Some(8080), 80, "tcp")],
        )];
        let wanted = wanted_forwards(&list);
        assert_eq!(wanted.len(), 1);
        let f = wanted.iter().next().unwrap();
        assert_eq!(f.host_port, 8080);
        // Docker published 80 onto the guest's 8080; 80 is only bound inside
        // the container's namespace.
        assert_eq!(f.guest_port, 8080);
    }

    #[test]
    fn wildcard_addresses_bind_loopback_not_every_interface() {
        // Binding 0.0.0.0 would expose the container to the local network,
        // which is not what Docker Desktop does and not what the user asked.
        for wildcard in ["0.0.0.0", "::"] {
            assert_eq!(
                normalize_bind(Some(wildcard)),
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            );
        }
        assert_eq!(normalize_bind(None), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn an_explicit_address_is_honored() {
        assert_eq!(
            normalize_bind(Some("192.168.1.5")),
            "192.168.1.5".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn an_unparseable_address_falls_back_to_loopback_rather_than_failing() {
        assert_eq!(
            normalize_bind(Some("not-an-address")),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }

    #[test]
    fn unpublished_ports_are_not_forwarded() {
        let list = vec![container(
            ContainerState::Running,
            vec![port(None, None, 9000, "tcp")],
        )];
        assert!(wanted_forwards(&list).is_empty());
    }

    #[test]
    fn stopped_containers_release_their_host_ports() {
        let list = vec![container(
            ContainerState::Exited,
            vec![port(Some("0.0.0.0"), Some(8080), 80, "tcp")],
        )];
        assert!(
            wanted_forwards(&list).is_empty(),
            "holding the port would block whatever wants it next"
        );
    }

    #[test]
    fn udp_is_skipped_rather_than_silently_forwarded_as_a_stream() {
        let list = vec![container(
            ContainerState::Running,
            vec![port(Some("0.0.0.0"), Some(5353), 53, "udp")],
        )];
        assert!(wanted_forwards(&list).is_empty());
    }

    #[test]
    fn duplicate_mappings_across_interfaces_collapse_to_one_listener() {
        // Docker reports one entry per bound address; binding twice would fail
        // with "address in use" against ourselves.
        let list = vec![container(
            ContainerState::Running,
            vec![
                port(Some("0.0.0.0"), Some(8080), 80, "tcp"),
                port(Some("::"), Some(8080), 80, "tcp"),
            ],
        )];
        assert_eq!(wanted_forwards(&list).len(), 1);
    }

    #[test]
    fn several_containers_each_get_their_mappings() {
        let list = vec![
            container(
                ContainerState::Running,
                vec![port(Some("0.0.0.0"), Some(8080), 80, "tcp")],
            ),
            container(
                ContainerState::Running,
                vec![port(Some("0.0.0.0"), Some(5432), 5432, "tcp")],
            ),
        ];
        assert_eq!(wanted_forwards(&list).len(), 2);
    }

    #[test]
    fn the_diff_starts_only_what_is_new_and_stops_only_what_is_gone() {
        let f = |host: u16, guest: u16| Forward {
            host_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            host_port: host,
            guest_port: guest,
            proto: "tcp".into(),
        };
        let current: BTreeSet<Forward> = [f(8080, 80), f(5432, 5432)].into_iter().collect();
        let wanted: BTreeSet<Forward> = [f(8080, 80), f(6379, 6379)].into_iter().collect();

        let (start, stop) = diff(&current, &wanted);
        assert_eq!(start, vec![f(6379, 6379)]);
        assert_eq!(stop, vec![f(5432, 5432)]);
    }

    #[test]
    fn an_unchanged_mapping_is_not_restarted() {
        // Tearing down and rebinding a live listener would drop connections on
        // every refresh of an unrelated container.
        let f = Forward {
            host_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            host_port: 8080,
            guest_port: 80,
            proto: "tcp".into(),
        };
        let set: BTreeSet<Forward> = [f].into_iter().collect();
        let (start, stop) = diff(&set, &set);
        assert!(start.is_empty());
        assert!(stop.is_empty());
    }

    #[test]
    fn the_agent_request_targets_the_published_port_inside_the_guest() {
        let f = Forward {
            host_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            host_port: 8080,
            guest_port: 8080,
            proto: "tcp".into(),
        };
        // Not 127.0.0.1:80 — nothing listens on 80 at the guest level.
        assert_eq!(f.agent_request(), "127.0.0.1:8080\n");
    }

    #[test]
    fn a_remapped_port_still_targets_the_published_side() {
        // `-p 9999:80` binds 9999 on the guest, not 80.
        let list = vec![container(
            ContainerState::Running,
            vec![port(Some("0.0.0.0"), Some(9999), 80, "tcp")],
        )];
        let f = wanted_forwards(&list).into_iter().next().unwrap();
        assert_eq!(f.host_port, 9999);
        assert_eq!(f.guest_port, 9999);
    }

    #[test]
    fn bind_failures_explain_what_to_do_about_them() {
        let f = Forward {
            host_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            host_port: 80,
            guest_port: 80,
            proto: "tcp".into(),
        };
        let in_use = std::io::Error::new(std::io::ErrorKind::AddrInUse, "x");
        assert!(bind_failure(&f, &in_use).contains("already in use"));

        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "x");
        assert!(bind_failure(&f, &denied).contains("below 1024"));
    }
}
