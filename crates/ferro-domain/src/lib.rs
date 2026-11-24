//! Domain types shared by every Ferro desktop application.

use std::path::{Path, PathBuf};

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
