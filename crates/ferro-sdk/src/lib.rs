//! Platform-neutral public types for Ferro applications.
//!
//! This crate deliberately contains no UI runtime or target SDK dependency. It is the stable
//! vocabulary shared by future Ferro project tooling and target adapters.

mod events;
mod menu;
mod window;

use std::fmt;
use std::str::FromStr;

pub use events::{AppEventProxy, EventSink};
pub use menu::{
    KeyModifiers, MenuItem, MenuItemKind, MenuSection, MenuShortcut, ShortcutKey, SpecialKey,
    StandardEditAction,
};
pub use window::{WindowDescriptor, WindowId, WindowLifecycle, WindowStyle};

/// A platform family supported by a Ferro application.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Target {
    /// Apple's macOS desktop platform.
    Macos,
    /// Microsoft's Windows desktop platform.
    Windows,
    /// Linux desktop distributions.
    Linux,
}

impl Target {
    /// Returns the canonical manifest spelling for this target.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    /// Returns every target known by this SDK version.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Macos, Self::Windows, Self::Linux]
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An error returned when a target name is not recognised.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseTargetError {
    value: String,
}

impl fmt::Display for ParseTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown Ferro target `{}`", self.value)
    }
}

impl std::error::Error for ParseTargetError {}

impl FromStr for Target {
    type Err = ParseTargetError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "macos" => Ok(Self::Macos),
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            _ => Err(ParseTargetError {
                value: value.to_owned(),
            }),
        }
    }
}

/// Stable metadata identifying a Ferro application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationDescriptor {
    name: String,
    id: String,
}

impl ApplicationDescriptor {
    /// Creates a descriptor from a human-readable name and a stable application identifier.
    #[must_use]
    pub fn new(name: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: id.into(),
        }
    }

    /// Returns the application's display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the application's stable reverse-DNS identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Metadata entry point implemented by a Ferro application.
///
/// Target runtimes obtain portable application metadata through this trait, then retain ownership
/// of native lifecycle and presentation behaviour.
pub trait Application {
    /// State rendered by native target hosts.
    type State;
    /// Input accepted by the application's reducer.
    type Event;
    /// Target work requested by the reducer after a state transition.
    type Effect;

    /// Returns the application's stable descriptor.
    fn descriptor() -> ApplicationDescriptor;

    /// Returns the state used before the first lifecycle event.
    fn initial_state() -> Self::State;

    /// Applies an event and returns ordered target effects.
    fn reduce(state: &mut Self::State, event: Self::Event) -> Vec<Self::Effect>;
}

/// Lifecycle phase emitted by a native application runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    /// The native runtime completed its initial launch.
    Launched,
    /// The application became active.
    Activated,
    /// The application stopped being active.
    Deactivated,
    /// The application is about to terminate.
    Terminating,
    /// The system appearance (e.g. Light/Dark) changed.
    AppearanceChanged,
}

/// Public category for a failure reported by Ferro services or runtimes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FerroErrorKind {
    /// A native runtime could not complete an operation.
    Runtime,
    /// A shared application service failed.
    Service,
    /// A native dialogue could not be presented or completed.
    Dialog,
    /// Optional persistence could not complete an operation.
    Persistence,
    /// A native capability was unavailable because the user or system denied permission.
    Permission,
}

/// Stable error returned by Ferro's public developer API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FerroError {
    kind: FerroErrorKind,
    message: String,
}

impl FerroError {
    /// Creates a categorized diagnostic safe to show in a development UI.
    #[must_use]
    pub fn new(kind: FerroErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Returns the category that produced this error.
    #[must_use]
    pub const fn kind(&self) -> FerroErrorKind {
        self.kind
    }
}

impl fmt::Display for FerroError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for FerroError {}

#[cfg(test)]
mod tests {
    use super::{
        AppEventProxy, Application, ApplicationDescriptor, FerroError, FerroErrorKind,
        KeyModifiers, MenuItem, MenuSection, MenuShortcut, SpecialKey, StandardEditAction, Target,
        WindowDescriptor, WindowId, WindowStyle,
    };

    struct Atlas;

    impl Application for Atlas {
        type State = usize;
        type Event = usize;
        type Effect = &'static str;

        fn descriptor() -> ApplicationDescriptor {
            ApplicationDescriptor::new("Atlas", "dev.ferro.atlas")
        }

        fn initial_state() -> Self::State {
            0
        }

        fn reduce(state: &mut Self::State, event: Self::Event) -> Vec<Self::Effect> {
            *state += event;
            vec!["render"]
        }
    }

    #[test]
    fn parses_known_desktop_targets() {
        assert_eq!("macos".parse::<Target>(), Ok(Target::Macos));
        assert_eq!("windows".parse::<Target>(), Ok(Target::Windows));
        assert_eq!("linux".parse::<Target>(), Ok(Target::Linux));
    }

    #[test]
    fn rejects_unknown_targets() {
        assert_eq!(
            "ios".parse::<Target>().unwrap_err().to_string(),
            "unknown Ferro target `ios`"
        );
    }

    #[test]
    fn applications_expose_portable_metadata() {
        assert_eq!(Atlas::descriptor().name(), "Atlas");
        assert_eq!(Atlas::descriptor().id(), "dev.ferro.atlas");
    }

    #[test]
    fn applications_expose_a_pure_state_transition() {
        let mut state = Atlas::initial_state();
        assert_eq!(Atlas::reduce(&mut state, 2), ["render"]);
        assert_eq!(state, 2);
    }

    #[test]
    fn errors_keep_a_stable_category() {
        let error = FerroError::new(FerroErrorKind::Persistence, "database unavailable");
        assert_eq!(error.kind(), FerroErrorKind::Persistence);
        assert!(error.to_string().contains("database unavailable"));
    }

    #[test]
    fn window_descriptors_capture_style_and_constraints() {
        let desc = WindowDescriptor::new(WindowId::main(), "Worklens")
            .size(1440.0, 960.0)
            .min_size(1000.0, 700.0)
            .always_on_top(true)
            .style(WindowStyle::Overlay {
                traffic_light_offset: Some((22.0, 28.0)),
            });
        assert!(desc.id.is_main());
        assert_eq!(desc.min_width, Some(1000.0));
        assert!(desc.always_on_top);
    }

    #[test]
    fn menu_sections_support_shortcuts_and_standard_edit() {
        let section = MenuSection::new(
            "Edit",
            vec![
                MenuItem::standard_edit(
                    "Copy",
                    StandardEditAction::Copy,
                    Some(MenuShortcut::cmd('c')),
                ),
                MenuItem::separator(),
                MenuItem::command("find", "Search", 42)
                    .shortcut(MenuShortcut::cmd_special(SpecialKey::Comma))
                    .enabled(true),
            ],
        );
        assert_eq!(section.items.len(), 3);
        assert!(KeyModifiers::COMMAND.with_shift().has_shift());
    }

    #[test]
    fn memory_event_proxy_records_events_across_threads() {
        let (proxy, log) = AppEventProxy::memory();
        let worker_proxy = proxy.clone();
        std::thread::spawn(move || {
            worker_proxy.send("bg-event").expect("send event");
        })
        .join()
        .expect("worker thread");

        assert_eq!(*log.lock().expect("lock"), vec!["bg-event"]);
    }
}
