use crate::config::HostsConfig;
use crate::views::host_grid::{GroupCard, HostCard};

/// Create group cards from hosts config
pub(super) fn group_cards(hosts_config: &HostsConfig) -> Vec<GroupCard> {
    hosts_config
        .groups
        .iter()
        .map(|group| {
            // Count hosts in this group
            let host_count = hosts_config
                .hosts
                .iter()
                .filter(|h| h.group_id == Some(group.id))
                .count();

            GroupCard {
                id: group.id,
                name: group.name.clone(),
                host_count,
            }
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
