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
