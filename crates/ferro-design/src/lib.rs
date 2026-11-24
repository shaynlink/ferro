//! Design intent shared across platforms, without sharing widgets.

/// Semantic density selected by the user or operating system.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Density {
    /// Uses the platform's ordinary spacing and target sizes.
    #[default]
    Standard,
    /// Uses compact spacing where the platform convention permits it.
    Compact,
}

/// Semantic spacing tokens; platforms map them to native metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Spacing {
    /// Nearby related controls.
    Tight,
    /// Ordinary grouped controls.
    Regular,
    /// Separate sections.
    Loose,
}
