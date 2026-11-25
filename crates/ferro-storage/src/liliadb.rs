use std::path::Path;

use ferro_contracts::{
    RecentWorkspaceStore, StoreError, WorkspaceActivityStore, WorkspaceIndex, WorkspaceMaintenance,
    WorkspaceTopology,
};
use ferro_domain::{
    Workspace, WorkspaceActivityPoint, WorkspaceArtifact, WorkspaceRelationship, WorkspaceSearchHit,
};
use lilia_db::{
    Batch, Database, FtsWrite, GraphWrite, Options, ResourceProfile, TimeSeriesPoint, Write,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

const CATALOG_SPACE: &str = "ferro_catalog_workspaces";
const ARTIFACT_SPACE: &str = "ferro_workspace_artifacts";
const ARTIFACT_SEARCH: &str = "ferro_workspace_artifacts";
const WORKSPACE_GRAPH: &str = "ferro_workspace_dependencies";

/// A durable global workspace catalogue backed by `LiliaDB` documents.
pub struct PersistentRecentWorkspaceStore {
    database: Database,
}

impl PersistentRecentWorkspaceStore {
    /// Opens or creates the global catalogue at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database directory cannot be created or opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self {
            database: open_database(path)?,
        })
    }
}

impl RecentWorkspaceStore for PersistentRecentWorkspaceStore {
    fn recent_workspaces(&self) -> Result<Vec<Workspace>, StoreError> {
        let entries = self
            .database
            .system()
            .json_scan(CATALOG_SPACE, &lilia_db::Scan::default())
            .map_err(storage_error)?;
        let mut records: Vec<WorkspaceRecord> = entries
            .into_iter()
            .map(|entry| serde_json::from_value(entry.value()).map_err(decode_error))
            .collect::<Result<_, _>>()?;
        records.sort_by_key(|record| std::cmp::Reverse(record.last_opened));
        records.into_iter().map(Workspace::try_from).collect()
    }

    fn remember_workspace(&mut self, workspace: &Workspace) -> Result<(), StoreError> {
        let value = serde_json::to_value(WorkspaceRecord::from(workspace))
            .map_err(|error| StoreError::new(format!("could not encode workspace: {error}")))?;
        self.database
            .system()
            .json_put(
                CATALOG_SPACE,
                &workspace.root().display().to_string(),
                &value,
            )
            .map(drop)
            .map_err(storage_error)
    }
}

/// A `LiliaDB` database dedicated to one workspace.
///
/// The adapter keeps `LiliaDB`'s multimodel writes atomic while exposing only Ferro domain values.
pub struct WorkspaceDatabase {
    database: Database,
}

impl WorkspaceDatabase {
    /// Opens or creates the workspace database at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database directory cannot be created or opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self {
            database: open_database(path)?,
        })
    }
}

impl WorkspaceIndex for WorkspaceDatabase {
    fn index_artifact(&mut self, artifact: &WorkspaceArtifact) -> Result<(), StoreError> {
        let document = json!({
            "id": artifact.id(),
            "path": artifact.path().to_string_lossy(),
            "content": artifact.content(),
        });
        let metadata = json!({ "path": artifact.path().to_string_lossy() });
        let mut batch = Batch::new();
        batch
            .json_put(ARTIFACT_SPACE, artifact.id(), &document)
            .map_err(storage_error)?;
        batch.push(Write::Fts(FtsWrite::Index {
            collection: ARTIFACT_SEARCH.to_owned(),
            doc_id: artifact.id().to_owned(),
            content: artifact.content().to_owned(),
        }));
        batch.push(Write::Graph(GraphWrite::SetNode {
            graph: WORKSPACE_GRAPH.to_owned(),
            id: artifact.id().to_owned(),
            kind: "artifact".to_owned(),
            metadata,
            document: Some((ARTIFACT_SPACE.to_owned(), artifact.id().to_owned())),
        }));
        self.database
            .system()
            .commit(&batch)
            .map_err(storage_error)?;
        Ok(())
    }

    fn search_artifacts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WorkspaceSearchHit>, StoreError> {
        self.database
            .system()
            .fts(ARTIFACT_SEARCH)
            .search(query, limit)
            .map(|hits| {
                hits.into_iter()
                    .map(|hit| WorkspaceSearchHit {
                        artifact_id: hit.doc_id,
                        snippet: hit.snippet,
                        score: hit.rank,
                    })
                    .collect()
            })
            .map_err(storage_error)
    }
}

impl WorkspaceTopology for WorkspaceDatabase {
    fn connect_artifacts(
        &mut self,
        relationship: &WorkspaceRelationship,
    ) -> Result<(), StoreError> {
        self.database
            .system()
            .graph(WORKSPACE_GRAPH)
            .add_edge(
                relationship.source(),
                relationship.target(),
                relationship.kind(),
                1.0,
            )
            .map(drop)
            .map_err(storage_error)
    }

    fn dependencies_of(&self, artifact_id: &str) -> Result<Vec<String>, StoreError> {
        self.database
            .system()
            .graph(WORKSPACE_GRAPH)
            .neighbors(artifact_id, lilia_db::Direction::Outbound, None)
            .map_err(storage_error)
    }
}

impl WorkspaceActivityStore for WorkspaceDatabase {
    fn record_activity(
        &mut self,
        series: &str,
        point: WorkspaceActivityPoint,
    ) -> Result<(), StoreError> {
        if !point.value.is_finite() {
            return Err(StoreError::new("workspace activity values must be finite"));
        }
        self.database
            .system()
            .series(series)
            .append(&[TimeSeriesPoint::new(point.timestamp_ms, point.value)])
            .map(drop)
            .map_err(storage_error)
    }
}

impl WorkspaceMaintenance for WorkspaceDatabase {
    fn quick_check(&self) -> Result<bool, StoreError> {
        self.database
            .check_integrity(true)
            .map(|report| report.ok)
            .map_err(storage_error)
    }

    fn integrity_check(&self) -> Result<bool, StoreError> {
        self.database
            .check_integrity(false)
            .map(|report| report.ok)
            .map_err(storage_error)
    }

    fn undo(&mut self) -> Result<bool, StoreError> {
        self.database
            .undo()
            .map(|step| step.is_some())
            .map_err(storage_error)
    }

    fn redo(&mut self) -> Result<bool, StoreError> {
        self.database
            .redo()
            .map(|step| step.is_some())
            .map_err(storage_error)
    }
}

#[derive(Deserialize, Serialize)]
struct WorkspaceRecord {
    name: String,
    root: String,
    last_opened: u64,
}

impl From<&Workspace> for WorkspaceRecord {
    fn from(workspace: &Workspace) -> Self {
        Self {
            name: workspace.name().to_owned(),
            root: workspace.root().to_string_lossy().into_owned(),
            last_opened: now_ms(),
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

fn open_database(path: impl AsRef<Path>) -> Result<Database, StoreError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            StoreError::new(format!("could not create storage directory: {error}"))
        })?;
    }
    let mut options = Options::for_profile(ResourceProfile::Constrained);
    options.undo_depth = 256;
    Database::open(path, options).map_err(storage_error)
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "map_err transfers ownership of the backend error to this adapter boundary"
)]
fn storage_error(error: lilia_db::Error) -> StoreError {
    StoreError::new(format!("LiliaDB operation failed: {error}"))
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "map_err transfers ownership of the serialization error to this adapter boundary"
)]
fn decode_error(error: serde_json::Error) -> StoreError {
    StoreError::new(format!("could not decode LiliaDB document: {error}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use ferro_contracts::{
        RecentWorkspaceStore, WorkspaceActivityStore, WorkspaceIndex, WorkspaceMaintenance,
        WorkspaceTopology,
    };
    use ferro_domain::{
        Workspace, WorkspaceActivityPoint, WorkspaceArtifact, WorkspaceRelationship,
    };

    use super::{PersistentRecentWorkspaceStore, WorkspaceDatabase};

    #[test]
    fn catalogue_survives_reopen() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("catalog.lilia");
        let workspace = Workspace::new("Ferro", "/workspace/ferro").expect("valid workspace");
        let mut store = PersistentRecentWorkspaceStore::open(&path).expect("open catalogue");
        store
            .remember_workspace(&workspace)
            .expect("persist workspace");
        drop(store);
        let reopened = PersistentRecentWorkspaceStore::open(&path).expect("reopen catalogue");
        assert_eq!(
            reopened.recent_workspaces().expect("load catalogue"),
            vec![workspace]
        );
    }

    #[test]
    fn workspace_models_share_one_database() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut database = WorkspaceDatabase::open(directory.path().join("workspace.lilia"))
            .expect("open workspace database");
        let source = WorkspaceArtifact::new("src", "src/lib.rs", "use ferro_domain::Workspace;")
            .expect("source artifact");
        let target = WorkspaceArtifact::new("domain", "crates/domain.rs", "pub struct Workspace;")
            .expect("target artifact");
        database.index_artifact(&source).expect("index source");
        database.index_artifact(&target).expect("index target");
        database
            .connect_artifacts(
                &WorkspaceRelationship::new("src", "domain", "imports").expect("edge"),
            )
            .expect("write edge");
        database
            .record_activity(
                "indexing",
                WorkspaceActivityPoint {
                    timestamp_ms: 1,
                    value: 2.0,
                },
            )
            .expect("record activity");
        assert_eq!(
            database
                .search_artifacts("Workspace", 10)
                .expect("search")
                .len(),
            2
        );
        assert_eq!(
            database.dependencies_of("src").expect("dependencies"),
            vec!["domain"]
        );
        assert!(database.quick_check().expect("quick check"));
        assert!(database.integrity_check().expect("integrity check"));
    }
}
