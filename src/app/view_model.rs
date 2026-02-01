use crate::config::HostsConfig;
use crate::views::host_grid::{GroupCard, HostCard};

/// Create group cards from hosts config
pub(super) fn group_cards(hosts_config: &HostsConfig) -> Vec<GroupCard> {
    hosts_config
        .groups
        .iter()
        .map(|group| GroupCard {
            id: group.id,
            name: group.name.clone(),
            collapsed: group.collapsed,
        })
        .collect()
}

/// Create host cards from hosts config
pub(super) fn host_cards(hosts_config: &HostsConfig) -> Vec<HostCard> {
    hosts_config
        .hosts
        .iter()
        .map(|host| HostCard {
            id: host.id,
            name: host.name.clone(),
            hostname: host.hostname.clone(),
            detected_os: host.detected_os.clone(),
            protocol: host.protocol.clone(),
            last_connected: host.last_connected,
            group_id: host.group_id,
        })
        .collect()
}

/// Filter host cards by search query
pub(super) fn filter_host_cards(query: &str, cards: Vec<HostCard>) -> Vec<HostCard> {
    if query.is_empty() {
        return cards;
    }

    let query = query.to_lowercase();
    cards
        .into_iter()
        .filter(|h| {
            h.name.to_lowercase().contains(&query) || h.hostname.to_lowercase().contains(&query)
        })
        .collect()
}

/// Filter group cards by search query
pub(super) fn filter_group_cards(query: &str, cards: Vec<GroupCard>) -> Vec<GroupCard> {
    if query.is_empty() {
        return cards;
    }

    let query = query.to_lowercase();
    cards
        .into_iter()
        .filter(|g| g.name.to_lowercase().contains(&query))
        .collect()
}
