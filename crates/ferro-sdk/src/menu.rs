//! Portable menu-bar and keyboard-shortcut descriptors.

/// Modifier flags associated with a keyboard shortcut.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    const BIT_COMMAND: u8 = 1 << 0;
    const BIT_SHIFT: u8 = 1 << 1;
    const BIT_OPTION: u8 = 1 << 2;
    const BIT_CONTROL: u8 = 1 << 3;

    /// No modifier keys pressed.
    pub const NONE: Self = Self(0);

    /// Primary command modifier (`Cmd` on macOS).
    pub const COMMAND: Self = Self(Self::BIT_COMMAND);

    /// Adds the primary command modifier.
    #[must_use]
    pub const fn with_command(self) -> Self {
        Self(self.0 | Self::BIT_COMMAND)
    }

    /// Adds the Shift modifier.
    #[must_use]
    pub const fn with_shift(self) -> Self {
        Self(self.0 | Self::BIT_SHIFT)
    }

    /// Adds the Option/Alt modifier.
    #[must_use]
    pub const fn with_option(self) -> Self {
        Self(self.0 | Self::BIT_OPTION)
    }

    /// Adds the Control modifier.
    #[must_use]
    pub const fn with_control(self) -> Self {
        Self(self.0 | Self::BIT_CONTROL)
    }

    /// Returns whether the primary command modifier is active.
    #[must_use]
    pub const fn has_command(self) -> bool {
        self.0 & Self::BIT_COMMAND != 0
    }

    /// Returns whether the Shift modifier is active.
    #[must_use]
    pub const fn has_shift(self) -> bool {
        self.0 & Self::BIT_SHIFT != 0
    }

    /// Returns whether the Option/Alt modifier is active.
    #[must_use]
    pub const fn has_option(self) -> bool {
        self.0 & Self::BIT_OPTION != 0
    }

    /// Returns whether the Control modifier is active.
    #[must_use]
    pub const fn has_control(self) -> bool {
        self.0 & Self::BIT_CONTROL != 0
    }
}

/// Named non-letter keys usable in menu shortcuts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecialKey {
    /// Escape key.
    Escape,
    /// Return / Enter key.
    Enter,
    /// Tab key.
    Tab,
    /// Backtick / grave accent key.
    Backtick,
    /// Comma key.
    Comma,
    /// Plus / equal key.
    Plus,
    /// Minus key.
    Minus,
}

/// Key binding attached to a menu item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuShortcut {
    /// Key kind triggered by this shortcut.
    pub key: ShortcutKey,
    /// Required modifier flags.
    pub modifiers: KeyModifiers,
}

/// Key token for a [`MenuShortcut`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutKey {
    /// Single ASCII character key equivalent.
    Char(char),
    /// Named special key.
    Special(SpecialKey),
}

impl MenuShortcut {
    /// Creates a `Cmd+<char>` shortcut.
    #[must_use]
    pub const fn cmd(key: char) -> Self {
        Self {
            key: ShortcutKey::Char(key),
            modifiers: KeyModifiers::COMMAND,
        }
    }

    /// Creates a `Cmd+<special>` shortcut.
    #[must_use]
    pub const fn cmd_special(key: SpecialKey) -> Self {
        Self {
            key: ShortcutKey::Special(key),
            modifiers: KeyModifiers::COMMAND,
        }
    }

    /// Creates a shortcut with explicit key and modifiers.
    #[must_use]
    pub const fn new(key: ShortcutKey, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }
}

/// Standard OS responder actions exposed by native edit menus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StandardEditAction {
    /// Undo last editing action.
    Undo,
    /// Redo last undone editing action.
    Redo,
    /// Cut current selection.
    Cut,
    /// Copy current selection.
    Copy,
    /// Paste from system clipboard.
    Paste,
    /// Select all text or items.
    SelectAll,
}

/// Behaviour represented by a menu item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuItemKind<E> {
    /// Application-defined command dispatching `event`.
    Command {
        /// Stable identifier.
        id: String,
        /// Event emitted when activated.
        event: E,
        /// Whether the menu item can be selected.
        enabled: bool,
    },
    /// Standard native responder action (`copy:`, `paste:`, etc.).
    StandardEdit(StandardEditAction),
    /// Visual separator line.
    Separator,
}

/// One entry in a top-level menu section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuItem<E> {
    /// Localized or human-readable label.
    pub title: String,
    /// Optional keyboard shortcut.
    pub shortcut: Option<MenuShortcut>,
    /// Item behaviour.
    pub kind: MenuItemKind<E>,
}

impl<E> MenuItem<E> {
    /// Creates an enabled application command item.
    #[must_use]
    pub fn command(id: impl Into<String>, title: impl Into<String>, event: E) -> Self {
        Self {
            title: title.into(),
            shortcut: None,
            kind: MenuItemKind::Command {
                id: id.into(),
                event,
                enabled: true,
            },
        }
    }

    /// Creates a standard responder edit item (Copy, Paste, Undo, etc.).
    #[must_use]
    pub fn standard_edit(
        title: impl Into<String>,
        action: StandardEditAction,
        shortcut: Option<MenuShortcut>,
    ) -> Self {
        Self {
            title: title.into(),
            shortcut,
            kind: MenuItemKind::StandardEdit(action),
        }
    }

    /// Creates a visual separator item.
    #[must_use]
    pub fn separator() -> Self {
        Self {
            title: String::new(),
            shortcut: None,
            kind: MenuItemKind::Separator,
        }
    }

    /// Attaches a keyboard shortcut to this menu item.
    #[must_use]
    pub const fn shortcut(mut self, shortcut: MenuShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Sets availability when this item is an application command.
    #[must_use]
    pub fn enabled(mut self, is_enabled: bool) -> Self {
        if let MenuItemKind::Command { enabled, .. } = &mut self.kind {
            *enabled = is_enabled;
        }
        self
    }
}

/// Top-level menu section rendered in the platform menu bar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuSection<E> {
    /// Menu title (e.g. `File`, `Edit`, `View`, `Window`).
    pub title: String,
    /// Ordered menu items.
    pub items: Vec<MenuItem<E>>,
}

impl<E> MenuSection<E> {
    /// Creates a menu section with the given title and items.
    #[must_use]
    pub fn new(title: impl Into<String>, items: Vec<MenuItem<E>>) -> Self {
        Self {
            title: title.into(),
            items,
        }
    }
}
