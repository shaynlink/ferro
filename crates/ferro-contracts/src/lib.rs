//! Platform-neutral boundaries between application logic and system services.

use ferro_domain::{
    Workspace, WorkspaceActivityPoint, WorkspaceArtifact, WorkspaceRelationship, WorkspaceSearchHit,
};
use ferro_sdk::FerroError;
use std::path::{Path, PathBuf};

/// A recently used file or directory retained by an application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentResource {
    path: PathBuf,
}

impl RecentResource {
    /// Creates a recent resource from a native path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the retained path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Optional application preferences service.
pub trait Preferences {
    /// Loads a UTF-8 preference value, or `None` when no value is present.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the configured persistence service cannot read the value.
    fn get(&self, key: &str) -> Result<Option<String>, FerroError>;

    /// Stores a UTF-8 preference value under `key`.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the configured persistence service cannot write the value.
    fn set(&mut self, key: &str, value: &str) -> Result<(), FerroError>;

    /// Removes the value at `key`; a missing key is successful.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the configured persistence service cannot remove the value.
    fn remove(&mut self, key: &str) -> Result<(), FerroError>;
}

/// Optional most-recently-used file and folder service.
pub trait RecentResources {
    /// Returns resources in most-recent-first order.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the configured persistence service cannot read recents.
    fn recent(&self) -> Result<Vec<RecentResource>, FerroError>;

    /// Records a resource as most recently used.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the configured persistence service cannot write recents.
    fn remember(&mut self, resource: RecentResource) -> Result<(), FerroError>;
}

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

/// Persistence boundary for text resources indexed inside one workspace.
pub trait WorkspaceIndex {
    /// Adds or replaces an artifact and updates its local searchable representation.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the artifact cannot be persisted or indexed.
    fn index_artifact(&mut self, artifact: &WorkspaceArtifact) -> Result<(), StoreError>;

    /// Searches the workspace-local index, most relevant first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the local index cannot be read.
    fn search_artifacts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WorkspaceSearchHit>, StoreError>;
}

/// Persistence boundary for workspace resource relationships.
pub trait WorkspaceTopology {
    /// Records a directed relationship between two indexed artifacts.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the relationship cannot be persisted.
    fn connect_artifacts(&mut self, relationship: &WorkspaceRelationship)
    -> Result<(), StoreError>;

    /// Returns the direct dependencies of an artifact.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the dependency graph cannot be read.
    fn dependencies_of(&self, artifact_id: &str) -> Result<Vec<String>, StoreError>;
}

/// Persistence boundary for local workspace activity metrics.
pub trait WorkspaceActivityStore {
    /// Appends an activity measurement to a named local series.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the measurement is invalid or cannot be persisted.
    fn record_activity(
        &mut self,
        series: &str,
        point: WorkspaceActivityPoint,
    ) -> Result<(), StoreError>;
}

/// Maintenance actions available to a local workspace database.
pub trait WorkspaceMaintenance {
    /// Returns whether a fast integrity verification succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when verification cannot run.
    fn quick_check(&self) -> Result<bool, StoreError>;

    /// Runs a complete integrity verification.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when verification cannot run.
    fn integrity_check(&self) -> Result<bool, StoreError>;

    /// Reverts the latest undoable workspace write, if one exists.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the history cannot be restored.
    fn undo(&mut self) -> Result<bool, StoreError>;

    /// Reapplies the latest reverted workspace write, if one exists.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the history cannot be restored.
    fn redo(&mut self) -> Result<bool, StoreError>;
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
