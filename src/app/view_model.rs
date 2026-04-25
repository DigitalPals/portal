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
    let query = query.trim();
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
    let query = query.trim();
    if query.is_empty() {
        return cards;
    }

    let query = query.to_lowercase();
    cards
        .into_iter()
        .filter(|g| g.name.to_lowercase().contains(&query))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DetectedOs, Protocol};
    use uuid::Uuid;

    fn host_card(name: &str, hostname: &str) -> HostCard {
        HostCard {
            id: Uuid::new_v4(),
            name: name.to_string(),
            hostname: hostname.to_string(),
            detected_os: Some(DetectedOs::Linux),
            protocol: Protocol::Ssh,
            last_connected: None,
            group_id: None,
        }
    }

    fn group_card(name: &str) -> GroupCard {
        GroupCard {
            id: Uuid::new_v4(),
            name: name.to_string(),
            collapsed: false,
        }
    }

    #[test]
    fn host_filter_trims_query() {
        let cards = vec![host_card("Production", "prod.example.com")];
        let filtered = filter_host_cards(" prod ", cards);

        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn group_filter_trims_query() {
        let cards = vec![group_card("Databases")];
        let filtered = filter_group_cards(" data ", cards);

        assert_eq!(filtered.len(), 1);
    }
}
