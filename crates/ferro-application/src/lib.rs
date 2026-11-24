//! Application use cases independent of a particular desktop operating system.

use ferro_contracts::{RecentWorkspaceStore, StoreError};
use ferro_domain::Workspace;

/// Loads the recent-workspace screen model.
///
/// # Errors
///
/// Returns [`StoreError`] when the configured persistence adapter cannot load recent workspaces.
pub fn load_recent_workspaces(
    store: &impl RecentWorkspaceStore,
) -> Result<RecentWorkspaces, StoreError> {
    Ok(RecentWorkspaces {
        workspaces: store.recent_workspaces()?,
    })
}

/// Data required by a native recent-workspaces screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentWorkspaces {
    /// Workspaces in display order.
    pub workspaces: Vec<Workspace>,
}

#[cfg(test)]
mod tests {
    use ferro_domain::Workspace;
    use ferro_testkit::MemoryRecentWorkspaceStore;

    use super::load_recent_workspaces;

    #[test]
    fn loads_recent_workspaces_in_store_order() {
        let first = Workspace::new("Ferro", "/workspace/ferro").expect("valid fixture");
        let second = Workspace::new("Atlas", "/workspace/atlas").expect("valid fixture");
        let store = MemoryRecentWorkspaceStore::with_workspaces(vec![first.clone(), second]);

        let model = load_recent_workspaces(&store).expect("the memory store cannot fail");

        assert_eq!(
            model.workspaces,
            vec![
                first,
                Workspace::new("Atlas", "/workspace/atlas").expect("valid fixture")
            ]
        );
    }
}
