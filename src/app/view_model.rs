use crate::config::hosts::HostGroup;
use crate::config::{Host, HostsConfig};
use crate::views::host_grid::{GroupCard, HostCard};

fn host_card(host: &Host) -> HostCard {
    HostCard {
        id: host.id,
        name: host.name.clone(),
        hostname: host.hostname.clone(),
        detected_os: host.detected_os.clone(),
        protocol: host.protocol.clone(),
        last_connected: host.last_connected,
        group_id: host.group_id,
    }
}

fn group_card(group: &HostGroup) -> GroupCard {
    GroupCard {
        id: group.id,
        name: group.name.clone(),
        collapsed: group.collapsed,
    }
}

/// Create group cards from hosts config
pub(super) fn group_cards(hosts_config: &HostsConfig) -> Vec<GroupCard> {
    hosts_config.groups.iter().map(group_card).collect()
}

/// Create host cards from hosts config
pub(super) fn host_cards(hosts_config: &HostsConfig) -> Vec<HostCard> {
    hosts_config.hosts.iter().map(host_card).collect()
}

/// Create host cards after applying the search filter, avoiding clones for filtered-out hosts.
pub(super) fn filtered_host_cards(query: &str, hosts_config: &HostsConfig) -> Vec<HostCard> {
    let query = query.trim();
    if query.is_empty() {
        return host_cards(hosts_config);
    }

    let query = query.to_lowercase();
    hosts_config
        .hosts
        .iter()
        .filter(|host| {
            host.name.to_lowercase().contains(&query)
                || host.hostname.to_lowercase().contains(&query)
        })
        .map(host_card)
        .collect()
}

/// Create group cards after applying the search filter, avoiding clones for filtered-out groups.
pub(super) fn filtered_group_cards(query: &str, hosts_config: &HostsConfig) -> Vec<GroupCard> {
    let query = query.trim();
    if query.is_empty() {
        return group_cards(hosts_config);
    }

    let query = query.to_lowercase();
    hosts_config
        .groups
        .iter()
        .filter(|group| group.name.to_lowercase().contains(&query))
        .map(group_card)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMethod, DetectedOs, Protocol};
    use chrono::Utc;
    use uuid::Uuid;

    fn host(name: &str, hostname: &str) -> Host {
        let now = Utc::now();
        Host {
            id: Uuid::new_v4(),
            name: name.to_string(),
            hostname: hostname.to_string(),
            port: 22,
            username: "root".to_string(),
            protocol: Protocol::Ssh,
            vnc_port: None,
            vnc_password_id: None,
            auth: AuthMethod::Agent,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: false,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            detected_os: Some(DetectedOs::Linux),
            last_connected: None,
        }
    }

    fn group(name: &str) -> HostGroup {
        HostGroup {
            id: Uuid::new_v4(),
            name: name.to_string(),
            parent_id: None,
            collapsed: false,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn host_filter_trims_query() {
        let config = HostsConfig {
            hosts: vec![host("Production", "prod.example.com")],
            groups: Vec::new(),
        };
        let filtered = filtered_host_cards(" prod ", &config);

        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn group_filter_trims_query() {
        let config = HostsConfig {
            hosts: Vec::new(),
            groups: vec![group("Databases")],
        };
        let filtered = filtered_group_cards(" data ", &config);

        assert_eq!(filtered.len(), 1);
    }
}
