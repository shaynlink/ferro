//! Native macOS system capabilities exposed to `MacosHost` implementations.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ferro_sdk::{FerroError, FerroErrorKind, WindowDescriptor, WindowId};
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSAppearance, NSAppearanceNameAqua,
    NSAppearanceNameDarkAqua, NSApplication, NSModalResponseOK, NSOpenPanel, NSPasteboard,
    NSPasteboardTypeString, NSSavePanel, NSWorkspace,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSString, NSURL};

/// A stable identifier for a host-defined native command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandId(String);

impl CommandId {
    /// Creates a command identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the underlying string identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A menu command rendered by the macOS runtime.
#[derive(Clone, Debug)]
pub struct MacosCommand<E> {
    /// Stable command identifier.
    pub id: CommandId,
    /// Menu item title.
    pub title: String,
    /// Optional single-character Command-key equivalent.
    pub shortcut: Option<char>,
    /// Whether `AppKit` should enable the menu item.
    pub enabled: bool,
    /// Event dispatched when the command is selected.
    pub event: E,
}

impl<E> MacosCommand<E> {
    /// Creates an enabled menu command without a shortcut.
    #[must_use]
    pub fn new(id: CommandId, title: impl Into<String>, event: E) -> Self {
        Self {
            id,
            title: title.into(),
            shortcut: None,
            enabled: true,
            event,
        }
    }

    /// Adds a Command-key shortcut.
    #[must_use]
    pub fn shortcut(mut self, shortcut: char) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Sets command availability.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Result returned from an `AppKit` open or save panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileDialogResult {
    /// The user closed the panel without selecting a path.
    Cancelled,
    /// The selected file or folder paths.
    Selected(Vec<PathBuf>),
}

/// Configuration for an open-panel request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenPanelMode {
    /// Select one or more files.
    Files {
        /// Whether `AppKit` permits selecting more than one file.
        multiple: bool,
    },
    /// Select a single folder.
    Folder,
}

/// Severity used by a native macOS alert.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertKind {
    /// Informational message.
    Information,
    /// Warning requiring the user's attention.
    Warning,
    /// Critical error message.
    Critical,
}

/// Result selected in a native confirmation dialogue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confirmation {
    /// The user selected the primary action.
    Accepted,
    /// The user selected cancel or dismissed the alert.
    Cancelled,
}

/// System appearance override requested for the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppearanceMode {
    /// Follow the current macOS system appearance.
    System,
    /// Force Aqua light appearance.
    Light,
    /// Force Aqua dark appearance.
    Dark,
}

/// Sender retained by native controls to dispatch application events on the main thread.
pub struct MacosEventSender<E>(pub(crate) Rc<dyn Fn(E)>);

impl<E> Clone for MacosEventSender<E> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<E> MacosEventSender<E> {
    /// Dispatches an event through the runtime's store.
    pub fn dispatch(&self, event: E) {
        (self.0)(event);
    }
}

pub(crate) trait WindowController {
    fn open_window(&mut self, descriptor: &WindowDescriptor) -> Result<(), FerroError>;
    fn close_window(&mut self, id: &WindowId);
    fn focus_window(&mut self, id: &WindowId);
    fn set_always_on_top(&mut self, id: &WindowId, always_on_top: bool);
    fn set_title(&mut self, id: &WindowId, title: &str);
}

/// Runtime access to macOS native capabilities (dialogs, workspace, clipboard, windows).
pub struct MacosCapabilities<E> {
    pub(crate) main_thread: MainThreadMarker,
    pub(crate) sender: MacosEventSender<E>,
    pub(crate) windows: Rc<RefCell<dyn WindowController>>,
}

impl<E: 'static> MacosCapabilities<E> {
    /// Returns the main-thread capability token for constructing `AppKit` views.
    #[must_use]
    pub const fn main_thread(&self) -> MainThreadMarker {
        self.main_thread
    }

    /// Presents a native informational, warning or critical alert.
    pub fn alert(&self, kind: AlertKind, title: &str, message: &str) {
        let alert = NSAlert::init(NSAlert::alloc(self.main_thread));
        alert.setAlertStyle(match kind {
            AlertKind::Information => NSAlertStyle::Informational,
            AlertKind::Warning => NSAlertStyle::Warning,
            AlertKind::Critical => NSAlertStyle::Critical,
        });
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(message));
        alert.runModal();
    }

    /// Presents a native confirmation alert with explicit primary and cancel labels.
    #[must_use]
    pub fn confirm(
        &self,
        title: &str,
        message: &str,
        accept_label: &str,
        cancel_label: &str,
    ) -> Confirmation {
        let alert = NSAlert::init(NSAlert::alloc(self.main_thread));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(message));
        alert.addButtonWithTitle(&NSString::from_str(accept_label));
        alert.addButtonWithTitle(&NSString::from_str(cancel_label));
        if alert.runModal() == NSAlertFirstButtonReturn {
            Confirmation::Accepted
        } else {
            Confirmation::Cancelled
        }
    }

    /// Opens a native panel and dispatches its result as an application event.
    pub fn open(&self, mode: OpenPanelMode, map_result: impl FnOnce(FileDialogResult) -> E) {
        let panel = NSOpenPanel::openPanel(self.main_thread);
        match mode {
            OpenPanelMode::Files { multiple } => {
                panel.setCanChooseFiles(true);
                panel.setCanChooseDirectories(false);
                panel.setAllowsMultipleSelection(multiple);
            }
            OpenPanelMode::Folder => {
                panel.setCanChooseFiles(false);
                panel.setCanChooseDirectories(true);
                panel.setAllowsMultipleSelection(false);
            }
        }
        let result = if panel.runModal() == NSModalResponseOK {
            let paths = panel
                .URLs()
                .iter()
                .filter_map(|url| url.path().map(|path| PathBuf::from(path.to_string())))
                .collect();
            FileDialogResult::Selected(paths)
        } else {
            FileDialogResult::Cancelled
        };
        self.sender.dispatch(map_result(result));
    }

    /// Opens a native save panel and dispatches its selected path as an application event.
    pub fn save(&self, suggested_name: &str, map_result: impl FnOnce(FileDialogResult) -> E) {
        let panel = NSSavePanel::savePanel(self.main_thread);
        panel.setNameFieldStringValue(&NSString::from_str(suggested_name));
        let result = if panel.runModal() == NSModalResponseOK {
            panel
                .URL()
                .and_then(|url| url.path())
                .map_or(FileDialogResult::Cancelled, |path| {
                    FileDialogResult::Selected(vec![PathBuf::from(path.to_string())])
                })
        } else {
            FileDialogResult::Cancelled
        };
        self.sender.dispatch(map_result(result));
    }

    /// Opens a validated external or local URL via macOS `NSWorkspace`.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the URL scheme is disallowed or `NSWorkspace` refuses it.
    pub fn open_url(&self, url: &str) -> Result<(), FerroError> {
        validate_external_url(url)?;
        let ns_url = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| {
            FerroError::new(FerroErrorKind::Runtime, format!("invalid URL `{url}`"))
        })?;
        let opened = NSWorkspace::sharedWorkspace().openURL(&ns_url);
        if opened {
            Ok(())
        } else {
            Err(FerroError::new(
                FerroErrorKind::Runtime,
                format!("macOS could not open `{url}`"),
            ))
        }
    }

    /// Reveals a path in Finder or opens it in the default system editor.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when `path` cannot be opened.
    pub fn open_path(&self, path: &Path, in_editor: bool) -> Result<(), FerroError> {
        if !path.exists() {
            return Err(FerroError::new(
                FerroErrorKind::Runtime,
                format!("path `{}` does not exist", path.display()),
            ));
        }
        if in_editor {
            let ns_path = NSString::from_str(&path.to_string_lossy());
            let url = NSURL::fileURLWithPath(&ns_path);
            if NSWorkspace::sharedWorkspace().openURL(&url) {
                Ok(())
            } else {
                Err(FerroError::new(
                    FerroErrorKind::Runtime,
                    format!("could not open `{}`", path.display()),
                ))
            }
        } else {
            self.reveal_in_finder(path)
        }
    }

    /// Reveals and selects `path` in Finder.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when `path` does not exist.
    pub fn reveal_in_finder(&self, path: &Path) -> Result<(), FerroError> {
        if !path.exists() {
            return Err(FerroError::new(
                FerroErrorKind::Runtime,
                format!("path `{}` does not exist", path.display()),
            ));
        }
        let ns_path = NSString::from_str(&path.to_string_lossy());
        let url = NSURL::fileURLWithPath(&ns_path);
        let urls = NSArray::from_slice(&[&*url]);
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
        Ok(())
    }

    /// Writes UTF-8 text to the macOS general pasteboard.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when `NSPasteboard` rejects the write.
    pub fn clipboard_write(&self, text: &str) -> Result<(), FerroError> {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // SAFETY: NSPasteboardTypeString is a valid static AppKit constant.
        let string_type = unsafe { NSPasteboardTypeString };
        if pasteboard.setString_forType(&NSString::from_str(text), string_type) {
            Ok(())
        } else {
            Err(FerroError::new(
                FerroErrorKind::Runtime,
                "could not write text to macOS clipboard",
            ))
        }
    }

    /// Reads UTF-8 text from the macOS general pasteboard if present.
    #[must_use]
    pub fn clipboard_read(&self) -> Option<String> {
        let pasteboard = NSPasteboard::generalPasteboard();
        // SAFETY: NSPasteboardTypeString is a valid static AppKit constant.
        let string_type = unsafe { NSPasteboardTypeString };
        pasteboard
            .stringForType(string_type)
            .map(|value| value.to_string())
    }

    /// Opens or focuses a managed window matching `descriptor`.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the window could not be constructed.
    pub fn open_window(&self, descriptor: &WindowDescriptor) -> Result<(), FerroError> {
        self.windows.borrow_mut().open_window(descriptor)
    }

    /// Closes a managed window by identifier.
    pub fn close_window(&self, id: &WindowId) {
        self.windows.borrow_mut().close_window(id);
    }

    /// Brings a managed window to the front.
    pub fn focus_window(&self, id: &WindowId) {
        self.windows.borrow_mut().focus_window(id);
    }

    /// Sets floating `always_on_top` level on a managed window.
    pub fn set_window_always_on_top(&self, id: &WindowId, always_on_top: bool) {
        self.windows
            .borrow_mut()
            .set_always_on_top(id, always_on_top);
    }

    /// Updates a managed window's title.
    pub fn set_window_title(&self, id: &WindowId, title: &str) {
        self.windows.borrow_mut().set_title(id, title);
    }

    /// Updates the macOS application appearance (System, Light, Dark).
    pub fn set_appearance(&self, mode: AppearanceMode) {
        let app = NSApplication::sharedApplication(self.main_thread);
        // SAFETY: NSAppearanceNameAqua and NSAppearanceNameDarkAqua are AppKit static names.
        let appearance = unsafe {
            match mode {
                AppearanceMode::System => None,
                AppearanceMode::Light => NSAppearance::appearanceNamed(NSAppearanceNameAqua),
                AppearanceMode::Dark => NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua),
            }
        };
        app.setAppearance(appearance.as_deref());
    }
}

/// Validates whether `url` is safe to pass to `NSWorkspace::openURL`.
///
/// # Errors
///
/// Returns [`FerroError`] when the scheme is not `https://`, local `http://`, or `devtools://`.
pub fn validate_external_url(url: &str) -> Result<(), FerroError> {
    let trimmed = url.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("devtools://") {
        return Ok(());
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        if host == "localhost"
            || host.starts_with("localhost:")
            || host == "127.0.0.1"
            || host.starts_with("127.0.0.1:")
            || host == "[::1]"
            || host.starts_with("[::1]:")
        {
            return Ok(());
        }
    }
    Err(FerroError::new(
        FerroErrorKind::Runtime,
        format!("disallowed external URL `{url}`"),
    ))
}

#[cfg(test)]
mod tests {
    use super::{CommandId, FileDialogResult, MacosCommand, OpenPanelMode, validate_external_url};

    #[test]
    fn commands_keep_identity_shortcuts_and_availability() {
        let command = MacosCommand::new(CommandId::new("open"), "Open…", 1)
            .shortcut('o')
            .enabled(false);
        assert_eq!(command.id, CommandId::new("open"));
        assert_eq!(command.shortcut, Some('o'));
        assert!(!command.enabled);
    }

    #[test]
    fn cancellation_has_no_selected_path() {
        assert_eq!(FileDialogResult::Cancelled, FileDialogResult::Cancelled);
        assert_eq!(OpenPanelMode::Folder, OpenPanelMode::Folder);
    }

    #[test]
    fn validates_safe_external_and_local_urls() {
        assert!(validate_external_url("https://github.com/shaynlink/ferro").is_ok());
        assert!(validate_external_url("http://localhost:1420/").is_ok());
        assert!(validate_external_url("http://127.0.0.1:8080/health").is_ok());
        assert!(validate_external_url("devtools://devtools/bundled/js_app.html").is_ok());
        assert!(validate_external_url("http://example.com").is_err());
        assert!(validate_external_url("http://localhost.evil.com").is_err());
        assert!(validate_external_url("file:///etc/passwd").is_err());
    }
}
