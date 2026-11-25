use std::path::Path;

use ferro_contracts::{RecentWorkspaceStore, StoreError};
use ferro_domain::Workspace;
use lilia_db::{Database, Options, ResourceProfile};
use serde::{Deserialize, Serialize};

const NAMESPACE: &str = "ferro";
const RECENT_WORKSPACES_KEY: &[u8] = b"recent-workspaces.v1";

/// A durable recent-workspace store backed by `LiliaDB`.
pub struct PersistentRecentWorkspaceStore {
    database: Database,
}

impl PersistentRecentWorkspaceStore {
    /// Opens or creates the `LiliaDB` database at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the storage directory cannot be created or `LiliaDB` cannot open
    /// the database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        ensure_parent_directory(path)?;
        let database = Database::open(path, Options::for_profile(ResourceProfile::Constrained))
            .map_err(|error| StoreError::new(format!("could not open LiliaDB: {error}")))?;
        Ok(Self { database })
    }
}

impl RecentWorkspaceStore for PersistentRecentWorkspaceStore {
    fn recent_workspaces(&self) -> Result<Vec<Workspace>, StoreError> {
        let entry = self
            .database
            .system()
            .kv_get(NAMESPACE, RECENT_WORKSPACES_KEY)
            .map_err(|error| StoreError::new(format!("could not read LiliaDB: {error}")))?;
        let Some(entry) = entry else {
            return Ok(Vec::new());
        };

        let records: Vec<WorkspaceRecord> =
            serde_json::from_slice(&entry.value).map_err(|error| {
                StoreError::new(format!("could not decode recent workspaces: {error}"))
            })?;
        records.into_iter().map(Workspace::try_from).collect()
    }

    fn remember_workspace(&mut self, workspace: &Workspace) -> Result<(), StoreError> {
        let mut workspaces = self.recent_workspaces()?;
        workspaces.retain(|item| item.root() != workspace.root());
        workspaces.insert(0, workspace.clone());
        let records: Vec<WorkspaceRecord> = workspaces.iter().map(WorkspaceRecord::from).collect();
        let value = serde_json::to_vec(&records).map_err(|error| {
            StoreError::new(format!("could not encode recent workspaces: {error}"))
        })?;

        self.database
            .system()
            .kv_put(NAMESPACE, RECENT_WORKSPACES_KEY, &value)
            .map_err(|error| StoreError::new(format!("could not write LiliaDB: {error}")))?;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct WorkspaceRecord {
    name: String,
    root: String,
}

impl From<&Workspace> for WorkspaceRecord {
    fn from(workspace: &Workspace) -> Self {
        Self {
            name: workspace.name().to_owned(),
            root: workspace.root().to_string_lossy().into_owned(),
        }
    }
}

impl TryFrom<WorkspaceRecord> for Workspace {
    type Error = StoreError;

    fn try_from(record: WorkspaceRecord) -> Result<Self, Self::Error> {
        Workspace::new(record.name, record.root)
            .map_err(|error| StoreError::new(format!("invalid stored workspace: {error}")))
    }
}

fn ensure_parent_directory(path: &Path) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            StoreError::new(format!("could not create storage directory: {error}"))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ferro_contracts::RecentWorkspaceStore;
    use ferro_domain::Workspace;

    use super::PersistentRecentWorkspaceStore;

    #[test]
    fn liliadb_store_survives_reopen() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("ferro.lilia");
        let workspace = Workspace::new("Ferro", "/workspace/ferro").expect("valid fixture");

        let mut store = PersistentRecentWorkspaceStore::open(&database_path).expect("open store");
        store
            .remember_workspace(&workspace)
            .expect("persist workspace");
        drop(store);

        let reopened = PersistentRecentWorkspaceStore::open(&database_path).expect("reopen store");
        assert_eq!(
            reopened.recent_workspaces().expect("load workspace"),
            vec![workspace]
        );
    }
}
