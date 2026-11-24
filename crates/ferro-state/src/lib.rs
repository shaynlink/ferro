//! Shared presentation state, deliberately separate from platform widgets.

use ferro_domain::Workspace;

/// A user intent emitted by a platform-native view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppIntent {
    /// The user selected a known workspace.
    OpenWorkspace(Workspace),
    /// The user asked to choose a new workspace from a native file dialog.
    ChooseWorkspace,
}

/// State common to the native desktop shells.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DesktopState {
    /// The workspaces currently shown by the shell.
    pub recent_workspaces: Vec<Workspace>,
}
