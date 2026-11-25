use std::path::Path;

use ferro_contracts::{RecentWorkspaceStore, StoreError};
use ferro_domain::Workspace;
use rusqlite::{Connection, params};

/// A durable recent-workspace store backed directly by SQLite.
pub struct PersistentRecentWorkspaceStore {
    connection: Connection,
}

impl PersistentRecentWorkspaceStore {
    /// Opens or creates the SQLite database at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the storage directory cannot be created, the database cannot
    /// be opened, or its schema cannot be initialized.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        ensure_parent_directory(path)?;
        let connection = Connection::open(path)
            .map_err(|error| StoreError::new(format!("could not open SQLite: {error}")))?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS recent_workspaces (
                    position INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    root TEXT NOT NULL UNIQUE
                );",
            )
            .map_err(|error| StoreError::new(format!("could not initialize SQLite: {error}")))?;
        Ok(Self { connection })
    }
}

impl RecentWorkspaceStore for PersistentRecentWorkspaceStore {
    fn recent_workspaces(&self) -> Result<Vec<Workspace>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT name, root FROM recent_workspaces ORDER BY position ASC")
            .map_err(|error| StoreError::new(format!("could not query SQLite: {error}")))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| StoreError::new(format!("could not query SQLite: {error}")))?;

        rows.map(|row| {
            let (name, root) = row
                .map_err(|error| StoreError::new(format!("could not read SQLite row: {error}")))?;
            Workspace::new(name, root)
                .map_err(|error| StoreError::new(format!("invalid stored workspace: {error}")))
        })
        .collect()
    }

    fn remember_workspace(&mut self, workspace: &Workspace) -> Result<(), StoreError> {
        let mut workspaces = self.recent_workspaces()?;
        workspaces.retain(|item| item.root() != workspace.root());
        workspaces.insert(0, workspace.clone());

        let transaction = self.connection.transaction().map_err(|error| {
            StoreError::new(format!("could not begin SQLite transaction: {error}"))
        })?;
        transaction
            .execute("DELETE FROM recent_workspaces", [])
            .map_err(|error| StoreError::new(format!("could not reset SQLite store: {error}")))?;
        for (position, item) in workspaces.iter().enumerate() {
            let position = i64::try_from(position)
                .map_err(|error| StoreError::new(format!("too many recent workspaces: {error}")))?;
            transaction
                .execute(
                    "INSERT INTO recent_workspaces (position, name, root) VALUES (?1, ?2, ?3)",
                    params![position, item.name(), item.root().to_string_lossy()],
                )
                .map_err(|error| StoreError::new(format!("could not write SQLite: {error}")))?;
        }
        transaction
            .commit()
            .map_err(|error| StoreError::new(format!("could not commit SQLite: {error}")))?;
        Ok(())
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
    fn sqlite_store_survives_reopen() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("ferro.sqlite");
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
