//! Domain types shared by every Ferro desktop application.

use std::path::{Path, PathBuf};

/// A text resource discovered in a workspace and available to local search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceArtifact {
    id: String,
    path: PathBuf,
    content: String,
}

impl WorkspaceArtifact {
    /// Creates an artifact with a stable workspace-local identifier.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceArtifactError::EmptyId`] or
    /// [`WorkspaceArtifactError::EmptyPath`] when the artifact cannot be addressed.
    pub fn new(
        id: impl Into<String>,
        path: impl Into<PathBuf>,
        content: impl Into<String>,
    ) -> Result<Self, WorkspaceArtifactError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(WorkspaceArtifactError::EmptyId);
        }
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(WorkspaceArtifactError::EmptyPath);
        }
        Ok(Self {
            id,
            path,
            content: content.into(),
        })
    }

    /// Returns the stable identifier of this artifact.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the artifact path relative to its workspace.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the text indexed for this artifact.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// A relationship between two resources in one workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRelationship {
    source: String,
    target: String,
    kind: String,
}

impl WorkspaceRelationship {
    /// Creates a relationship with non-empty endpoints and kind.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceRelationshipError::EmptyField`] when one field is blank.
    pub fn new(
        source: impl Into<String>,
        target: impl Into<String>,
        kind: impl Into<String>,
    ) -> Result<Self, WorkspaceRelationshipError> {
        let source = source.into();
        let target = target.into();
        let kind = kind.into();
        if source.trim().is_empty() || target.trim().is_empty() || kind.trim().is_empty() {
            return Err(WorkspaceRelationshipError::EmptyField);
        }
        Ok(Self {
            source,
            target,
            kind,
        })
    }

    /// Returns the source artifact identifier.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the target artifact identifier.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the relationship kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
}

/// A match returned by a local workspace search.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSearchHit {
    /// The matched artifact identifier.
    pub artifact_id: String,
    /// A human-readable excerpt of the match.
    pub snippet: String,
    /// The backend relevance score. Lower scores rank first for `LiliaDB` BM25.
    pub score: f64,
}

/// One numeric activity measurement recorded for a workspace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkspaceActivityPoint {
    /// Unix epoch milliseconds.
    pub timestamp_ms: i64,
    /// The measured value.
    pub value: f64,
}

/// A violated artifact invariant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceArtifactError {
    /// The artifact identifier contains no visible text.
    EmptyId,
    /// The artifact path has no path data.
    EmptyPath,
}

impl std::fmt::Display for WorkspaceArtifactError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyId => formatter.write_str("workspace artifact id cannot be empty"),
            Self::EmptyPath => formatter.write_str("workspace artifact path cannot be empty"),
        }
    }
}

impl std::error::Error for WorkspaceArtifactError {}

/// A violated workspace relationship invariant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceRelationshipError {
    /// At least one endpoint or relationship kind contains no visible text.
    EmptyField,
}

impl std::fmt::Display for WorkspaceRelationshipError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("workspace relationship fields cannot be empty")
    }
}

impl std::error::Error for WorkspaceRelationshipError {}

/// A user-visible workspace known by the application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    name: String,
    root: PathBuf,
}

impl Workspace {
    /// Creates a workspace after validating the name and root path.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError::EmptyName`] when the supplied name has no visible text, or
    /// [`WorkspaceError::EmptyRoot`] when the path is empty.
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Result<Self, WorkspaceError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(WorkspaceError::EmptyName);
        }

        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(WorkspaceError::EmptyRoot);
        }

        Ok(Self { name, root })
    }

    /// Returns the workspace display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the workspace root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// A violated workspace invariant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceError {
    /// The workspace name contains no visible text.
    EmptyName,
    /// The workspace root has no path data.
    EmptyRoot,
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("workspace name cannot be empty"),
            Self::EmptyRoot => formatter.write_str("workspace root cannot be empty"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

#[cfg(test)]
mod tests {
    use super::{Workspace, WorkspaceError};

    #[test]
    fn workspace_requires_a_name() {
        assert_eq!(
            Workspace::new("  ", "/tmp").unwrap_err(),
            WorkspaceError::EmptyName
        );
    }
}
