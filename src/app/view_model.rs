use crate::config::hosts::HostGroup;
use crate::config::{DetectedOs, Host, HostsConfig, Protocol};
use crate::views::host_grid::{GroupCard, HostCard};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

const HOST_GRID_CACHE_LOG_ITEM_THRESHOLD: usize = 200;
const HOST_GRID_CACHE_LOG_DURATION_THRESHOLD: Duration = Duration::from_millis(8);

fn host_card(host: &Host) -> HostCard {
    HostCard {
        id: host.id,
        name: host.name.clone(),
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
    let query = normalize_query(query);
    if query.is_empty() {
        return host_cards(hosts_config);
    }

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
    let query = normalize_query(query);
    if query.is_empty() {
        return group_cards(hosts_config);
    }

    hosts_config
        .groups
        .iter()
        .filter(|group| group.name.to_lowercase().contains(&query))
        .map(group_card)
        .collect()
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HostGridCards {
    pub groups: Vec<GroupCard>,
    pub hosts: Vec<HostCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HostGridCacheKey {
    query: String,
    signature: u64,
}

#[derive(Debug, Default)]
pub(crate) struct HostGridCache {
    key: Option<HostGridCacheKey>,
    cards: HostGridCards,
}

impl HostGridCache {
    pub fn cards(&mut self, query: &str, hosts_config: &HostsConfig) -> &HostGridCards {
        let key = HostGridCacheKey {
            query: normalize_query(query),
            signature: hosts_signature(hosts_config),
        };

        if self.key.as_ref() != Some(&key) {
            let started = Instant::now();
            self.cards = HostGridCards {
                groups: filtered_group_cards(&key.query, hosts_config),
                hosts: filtered_host_cards(&key.query, hosts_config),
            };
            let elapsed = started.elapsed();
            let item_count = hosts_config.hosts.len() + hosts_config.groups.len();
            if item_count >= HOST_GRID_CACHE_LOG_ITEM_THRESHOLD
                || elapsed >= HOST_GRID_CACHE_LOG_DURATION_THRESHOLD
            {
                tracing::debug!(
                    hosts = hosts_config.hosts.len(),
                    groups = hosts_config.groups.len(),
                    filtered_hosts = self.cards.hosts.len(),
                    filtered_groups = self.cards.groups.len(),
                    query_len = key.query.len(),
                    elapsed_ms = elapsed.as_millis(),
                    "rebuilt host-grid card cache"
                );
            }
            self.key = Some(key);
        }

        &self.cards
    }
}

fn normalize_query(query: &str) -> String {
    query.trim().to_lowercase()
}

fn hosts_signature(hosts_config: &HostsConfig) -> u64 {
    let mut hasher = DefaultHasher::new();
    hosts_config.hosts.len().hash(&mut hasher);
    hosts_config.groups.len().hash(&mut hasher);

    for host in &hosts_config.hosts {
        host.id.hash(&mut hasher);
        host.name.hash(&mut hasher);
        host.hostname.hash(&mut hasher);
        protocol_key(&host.protocol).hash(&mut hasher);
        host.group_id.hash(&mut hasher);
        hash_datetime(host.updated_at, &mut hasher);
        if let Some(last_connected) = host.last_connected {
            hash_datetime(last_connected, &mut hasher);
        }
        if let Some(os) = &host.detected_os {
            hash_detected_os(os, &mut hasher);
        }
    }

    for group in &hosts_config.groups {
        group.id.hash(&mut hasher);
        group.name.hash(&mut hasher);
        group.parent_id.hash(&mut hasher);
        group.collapsed.hash(&mut hasher);
    }

    hasher.finish()
}

fn hash_datetime(dt: chrono::DateTime<chrono::Utc>, hasher: &mut DefaultHasher) {
    dt.timestamp().hash(hasher);
    dt.timestamp_subsec_nanos().hash(hasher);
}

fn protocol_key(protocol: &Protocol) -> u8 {
    match protocol {
        Protocol::Ssh => 0,
        Protocol::Vnc => 1,
    }
}

fn hash_detected_os(os: &DetectedOs, hasher: &mut DefaultHasher) {
    std::mem::discriminant(os).hash(hasher);
    if let DetectedOs::Unknown(value) = os {
        value.hash(hasher);
    }
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

    #[test]
    fn host_grid_cache_reuses_trimmed_query() {
        let config = HostsConfig {
            hosts: vec![host("Production", "prod.example.com")],
            groups: Vec::new(),
        };
        let mut cache = HostGridCache::default();

        cache.cards(" prod ", &config);
        let first_key = cache.key.clone();

        cache.cards("prod", &config);

        assert_eq!(cache.key, first_key);
        assert_eq!(cache.cards.hosts.len(), 1);
    }

    #[test]
    fn host_grid_cache_invalidates_when_hosts_change() {
        let mut config = HostsConfig {
            hosts: vec![host("Production", "prod.example.com")],
            groups: Vec::new(),
        };
        let mut cache = HostGridCache::default();

        cache.cards("prod", &config);
        let first_key = cache.key.clone();

        config
            .hosts
            .push(host("Production Backup", "prod-b.example.com"));
        cache.cards("prod", &config);

        assert_ne!(cache.key, first_key);
        assert_eq!(cache.cards.hosts.len(), 2);
    }
}
