use crate::config::HostsConfig;
use crate::views::host_grid::HostCard;
use crate::views::sidebar::{SidebarFolder, SidebarHost};

pub(super) fn sidebar_hosts(hosts_config: &HostsConfig) -> Vec<SidebarHost> {
    hosts_config
        .hosts
        .iter()
        .map(|host| {
            let folder_name = host.group_id.and_then(|gid| {
                hosts_config
                    .find_group(gid)
                    .map(|g| g.name.clone())
            });
            SidebarHost {
                id: host.id,
                name: host.name.clone(),
                hostname: host.hostname.clone(),
                folder: folder_name,
            }
        })
        .collect()
}

pub(super) fn sidebar_folders(hosts_config: &HostsConfig) -> Vec<SidebarFolder> {
    hosts_config
        .groups
        .iter()
        .map(|group| SidebarFolder {
            id: group.id,
            name: group.name.clone(),
            expanded: !group.collapsed,
        })
        .collect()
}

pub(super) fn host_cards(hosts_config: &HostsConfig) -> Vec<HostCard> {
    hosts_config
        .hosts
        .iter()
        .map(|host| HostCard {
            id: host.id,
            name: host.name.clone(),
            hostname: host.hostname.clone(),
            username: host.username.clone(),
            tags: host.tags.clone(),
            last_connected: None,
        })
        .collect()
}

pub(super) fn filter_sidebar_hosts(query: &str, hosts: Vec<SidebarHost>) -> Vec<SidebarHost> {
    if query.is_empty() {
        return hosts;
    }

    let query = query.to_lowercase();
    hosts
        .into_iter()
        .filter(|h| h.name.to_lowercase().contains(&query) || h.hostname.to_lowercase().contains(&query))
        .collect()
}

pub(super) fn filter_host_cards(query: &str, cards: Vec<HostCard>) -> Vec<HostCard> {
    if query.is_empty() {
        return cards;
    }

    let query = query.to_lowercase();
    cards
        .into_iter()
        .filter(|h| h.name.to_lowercase().contains(&query) || h.hostname.to_lowercase().contains(&query))
        .collect()
}
