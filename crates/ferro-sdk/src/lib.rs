//! Platform-neutral public types for Ferro applications.
//!
//! This crate deliberately contains no UI runtime or target SDK dependency. It is the stable
//! vocabulary shared by future Ferro project tooling and target adapters.

use std::fmt;
use std::str::FromStr;

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
}

#[cfg(test)]
mod tests {
    use super::{Application, ApplicationDescriptor, Target};

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
}
