//! Platform-neutral boundaries between application logic and system services.

use ferro_domain::Workspace;

/// Persistence boundary for the user's recently opened workspaces.
pub trait RecentWorkspaceStore {
    /// Returns workspaces in most-recent-first order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the platform persistence adapter cannot read its backing store.
    fn recent_workspaces(&self) -> Result<Vec<Workspace>, StoreError>;

    /// Records a workspace as recently opened.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the platform persistence adapter cannot write its backing store.
    fn remember_workspace(&mut self, workspace: &Workspace) -> Result<(), StoreError>;
}

/// Persistence failure that is safe to show in a diagnostic screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreError {
    message: String,
}

impl StoreError {
    /// Creates a store failure from a human-readable diagnostic.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}
