//! Test-only adapters shared by workspace crates.

use ferro_contracts::{RecentWorkspaceStore, StoreError};
use ferro_domain::Workspace;

/// In-memory recent-workspace store for deterministic tests.
#[derive(Clone, Debug, Default)]
pub struct MemoryRecentWorkspaceStore {
    workspaces: Vec<Workspace>,
}

impl MemoryRecentWorkspaceStore {
    /// Builds a store with a known most-recent-first sequence.
    #[must_use]
    pub fn with_workspaces(workspaces: Vec<Workspace>) -> Self {
        Self { workspaces }
    }
}

impl RecentWorkspaceStore for MemoryRecentWorkspaceStore {
    fn recent_workspaces(&self) -> Result<Vec<Workspace>, StoreError> {
        Ok(self.workspaces.clone())
    }

    fn remember_workspace(&mut self, workspace: &Workspace) -> Result<(), StoreError> {
        self.workspaces
            .retain(|item| item.root() != workspace.root());
        self.workspaces.insert(0, workspace.clone());
        Ok(())
    }
}
