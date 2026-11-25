//! Platform-neutral window descriptors for multi-window Ferro hosts.

use std::fmt;

/// Stable identifier for a window managed by a Ferro target runtime.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowId(String);

impl WindowId {
    /// Identifier reserved for the application's primary window.
    pub const MAIN_ID: &'static str = "main";

    /// Creates a window identifier from an arbitrary non-empty label.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the primary application window identifier.
    #[must_use]
    pub fn main() -> Self {
        Self(Self::MAIN_ID.to_owned())
    }

    /// Returns the underlying string identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether this identifier refers to the primary window.
    #[must_use]
    pub fn is_main(&self) -> bool {
        self.0 == Self::MAIN_ID
    }
}

impl Default for WindowId {
    fn default() -> Self {
        Self::main()
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Visual chrome and window-behavior style requested from a native host.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum WindowStyle {
    /// Standard OS-managed titlebar and frame.
    #[default]
    Standard,
    /// Content extends beneath a transparent titlebar with optional traffic-light offset.
    Overlay {
        /// Optional `(x, y)` point offset applied to native window controls.
        traffic_light_offset: Option<(f64, f64)>,
    },
    /// Lightweight floating HUD / widget panel.
    HudPanel {
        /// Whether the panel floats without stealing activation from other applications.
        non_activating: bool,
    },
}

/// Portable creation and update configuration for a native window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowDescriptor {
    /// Stable window identity.
    pub id: WindowId,
    /// Window title displayed by the operating system.
    pub title: String,
    /// Initial content width in logical points.
    pub width: f64,
    /// Initial content height in logical points.
    pub height: f64,
    /// Optional minimum width in logical points.
    pub min_width: Option<f64>,
    /// Optional minimum height in logical points.
    pub min_height: Option<f64>,
    /// Optional initial X coordinate in screen points.
    pub x: Option<f64>,
    /// Optional initial Y coordinate in screen points.
    pub y: Option<f64>,
    /// Whether the user may resize the window.
    pub resizable: bool,
    /// Whether the window floats above standard windows.
    pub always_on_top: bool,
    /// Chrome and activation style.
    pub style: WindowStyle,
}

impl WindowDescriptor {
    /// Creates a resizable standard window descriptor with default `800 x 520` dimensions.
    #[must_use]
    pub fn new(id: WindowId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            width: 800.0,
            height: 520.0,
            min_width: None,
            min_height: None,
            x: None,
            y: None,
            resizable: true,
            always_on_top: false,
            style: WindowStyle::Standard,
        }
    }

    /// Sets the initial content dimensions in logical points.
    #[must_use]
    pub const fn size(mut self, width: f64, height: f64) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets minimum content dimensions in logical points.
    #[must_use]
    pub const fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_width = Some(width);
        self.min_height = Some(height);
        self
    }

    /// Sets the initial screen origin in logical points.
    #[must_use]
    pub const fn position(mut self, x: f64, y: f64) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self
    }

    /// Sets whether the window is resizable.
    #[must_use]
    pub const fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Sets whether the window remains floating above ordinary windows.
    #[must_use]
    pub const fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    /// Sets the window chrome and activation style.
    #[must_use]
    pub const fn style(mut self, style: WindowStyle) -> Self {
        self.style = style;
        self
    }
}

/// Window-scoped lifecycle notification emitted by a native runtime.
#[derive(Clone, Debug, PartialEq)]
pub enum WindowLifecycle {
    /// A managed window became the key/focused window.
    Focused(WindowId),
    /// A managed window was closed by the user or OS.
    Closed(WindowId),
    /// A managed window was resized to the given logical content dimensions.
    Resized {
        /// Window identifier.
        id: WindowId,
        /// New content width in logical points.
        width: f64,
        /// New content height in logical points.
        height: f64,
    },
}
