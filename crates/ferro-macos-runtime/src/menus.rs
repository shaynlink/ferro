//! Native macOS menu bar construction from Ferro `MenuSection` and `MacosCommand` models.

use ferro_sdk::{
    KeyModifiers, MenuItem, MenuItemKind, MenuSection, MenuShortcut, ShortcutKey, SpecialKey,
    StandardEditAction,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadOnly, sel};
use objc2_app_kit::{NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSInteger, NSString};

use crate::capabilities::MacosCommand;

pub(crate) fn build_menu_bar<E: Clone>(
    main_thread: MainThreadMarker,
    app_name: &str,
    sections: &[MenuSection<E>],
    legacy_commands: &[MacosCommand<E>],
    router: &AnyObject,
    include_devtools_menu: bool,
) -> (Retained<NSMenu>, Vec<E>) {
    let main_menu = NSMenu::initWithTitle(NSMenu::alloc(main_thread), &NSString::from_str("Main"));
    let mut dispatched = Vec::new();

    add_app_menu(main_thread, &main_menu, app_name);

    if sections.is_empty() {
        let items = legacy_commands
            .iter()
            .map(|cmd| {
                let mut item = MenuItem::command(cmd.id.as_str(), &cmd.title, cmd.event.clone())
                    .enabled(cmd.enabled);
                if let Some(ch) = cmd.shortcut {
                    item = item.shortcut(MenuShortcut::cmd(ch));
                }
                item
            })
            .collect();
        let file_section = MenuSection::new("File", items);
        add_section(
            main_thread,
            &main_menu,
            &file_section,
            router,
            &mut dispatched,
        );
    } else {
        for section in sections {
            add_section(main_thread, &main_menu, section, router, &mut dispatched);
        }
    }

    if include_devtools_menu {
        add_devtools_section(main_thread, &main_menu, router);
    }

    (main_menu, dispatched)
}

fn add_devtools_section(main_thread: MainThreadMarker, main_menu: &NSMenu, router: &AnyObject) {
    let submenu = NSMenu::initWithTitle(NSMenu::alloc(main_thread), &NSString::from_str("Debug"));
    submenu.setAutoenablesItems(false);

    let items = [
        ("Toggle Ferro DevTools", "d", -1_isize),
        ("Trigger Hot Reload", "r", -2_isize),
    ];
    for (title, key, tag) in items {
        // SAFETY: `performDevCommand:` is defined on `CommandRouter`, retained by `Driver`.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(main_thread),
                &NSString::from_str(title),
                Some(sel!(performDevCommand:)),
                &NSString::from_str(key),
            )
        };
        item.setTag(tag);
        item.setKeyEquivalentModifierMask(
            NSEventModifierFlags::Command.union(NSEventModifierFlags::Option),
        );
        // SAFETY: `router` outlives menu interactions while `Driver` runs.
        unsafe { item.setTarget(Some(router)) };
        submenu.addItem(&item);
    }

    // SAFETY: Submenu parent item requires no action selector.
    let section_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(main_thread),
            &NSString::from_str("Debug"),
            None,
            &NSString::from_str(""),
        )
    };
    section_item.setSubmenu(Some(&submenu));
    main_menu.addItem(&section_item);
}

fn add_app_menu(main_thread: MainThreadMarker, main_menu: &NSMenu, app_name: &str) {
    let app_menu = NSMenu::initWithTitle(NSMenu::alloc(main_thread), &NSString::from_str(app_name));
    // SAFETY: Standard Cocoa responder selector `terminate:` exists on NSApplication.
    let quit_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(main_thread),
            &NSString::from_str(&format!("Quit {app_name}")),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    app_menu.addItem(&quit_item);

    // SAFETY: Submenu parent item requires no target selector.
    let root_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(main_thread),
            &NSString::from_str(app_name),
            None,
            &NSString::from_str(""),
        )
    };
    root_item.setSubmenu(Some(&app_menu));
    main_menu.addItem(&root_item);
}

fn add_section<E: Clone>(
    main_thread: MainThreadMarker,
    main_menu: &NSMenu,
    section: &MenuSection<E>,
    router: &AnyObject,
    dispatched: &mut Vec<E>,
) {
    let submenu = NSMenu::initWithTitle(
        NSMenu::alloc(main_thread),
        &NSString::from_str(&section.title),
    );
    submenu.setAutoenablesItems(false);

    for entry in &section.items {
        match &entry.kind {
            MenuItemKind::Separator => {
                submenu.addItem(&NSMenuItem::separatorItem(main_thread));
            }
            MenuItemKind::StandardEdit(action) => {
                let (key, mask) = shortcut_to_appkit(entry.shortcut);
                // SAFETY: Standard Cocoa Edit responder selectors (`undo:`, `copy:`, etc.).
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(main_thread),
                        &NSString::from_str(&entry.title),
                        Some(standard_edit_selector(*action)),
                        &NSString::from_str(&key),
                    )
                };
                if entry.shortcut.is_some() {
                    item.setKeyEquivalentModifierMask(mask);
                }
                submenu.addItem(&item);
            }
            MenuItemKind::Command { event, enabled, .. } => {
                let index = dispatched.len();
                let Ok(tag) = NSInteger::try_from(index) else {
                    continue;
                };
                dispatched.push(event.clone());
                let (key, mask) = shortcut_to_appkit(entry.shortcut);
                // SAFETY: `performCommand:` is defined on `CommandRouter`, retained by `Driver`.
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(main_thread),
                        &NSString::from_str(&entry.title),
                        Some(sel!(performCommand:)),
                        &NSString::from_str(&key),
                    )
                };
                item.setTag(tag);
                item.setEnabled(*enabled);
                if entry.shortcut.is_some() {
                    item.setKeyEquivalentModifierMask(mask);
                }
                // SAFETY: `router` outlives menu interactions while `Driver` runs.
                unsafe { item.setTarget(Some(router)) };
                submenu.addItem(&item);
            }
        }
    }

    // SAFETY: Submenu parent item requires no action selector.
    let section_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(main_thread),
            &NSString::from_str(&section.title),
            None,
            &NSString::from_str(""),
        )
    };
    section_item.setSubmenu(Some(&submenu));
    main_menu.addItem(&section_item);
}

fn standard_edit_selector(action: StandardEditAction) -> Sel {
    match action {
        StandardEditAction::Undo => sel!(undo:),
        StandardEditAction::Redo => sel!(redo:),
        StandardEditAction::Cut => sel!(cut:),
        StandardEditAction::Copy => sel!(copy:),
        StandardEditAction::Paste => sel!(paste:),
        StandardEditAction::SelectAll => sel!(selectAll:),
    }
}

fn shortcut_to_appkit(shortcut: Option<MenuShortcut>) -> (String, NSEventModifierFlags) {
    let Some(shortcut) = shortcut else {
        return (String::new(), NSEventModifierFlags::empty());
    };
    let key = match shortcut.key {
        ShortcutKey::Char(ch) => ch.to_ascii_lowercase().to_string(),
        ShortcutKey::Special(SpecialKey::Escape) => "\x1b".to_owned(),
        ShortcutKey::Special(SpecialKey::Enter) => "\r".to_owned(),
        ShortcutKey::Special(SpecialKey::Tab) => "\t".to_owned(),
        ShortcutKey::Special(SpecialKey::Backtick) => "`".to_owned(),
        ShortcutKey::Special(SpecialKey::Comma) => ",".to_owned(),
        ShortcutKey::Special(SpecialKey::Plus) => "=".to_owned(),
        ShortcutKey::Special(SpecialKey::Minus) => "-".to_owned(),
    };
    (key, modifiers_to_flags(shortcut.modifiers))
}

const fn modifiers_to_flags(modifiers: KeyModifiers) -> NSEventModifierFlags {
    let mut flags = NSEventModifierFlags::empty();
    if modifiers.has_command() {
        flags = flags.union(NSEventModifierFlags::Command);
    }
    if modifiers.has_shift() {
        flags = flags.union(NSEventModifierFlags::Shift);
    }
    if modifiers.has_option() {
        flags = flags.union(NSEventModifierFlags::Option);
    }
    if modifiers.has_control() {
        flags = flags.union(NSEventModifierFlags::Control);
    }
    flags
}

#[cfg(test)]
mod tests {
    use ferro_sdk::{KeyModifiers, MenuShortcut, ShortcutKey, SpecialKey};
    use objc2_app_kit::NSEventModifierFlags;

    use super::shortcut_to_appkit;

    #[test]
    fn converts_special_keys_and_modifiers() {
        let (key, flags) = shortcut_to_appkit(Some(MenuShortcut::new(
            ShortcutKey::Special(SpecialKey::Backtick),
            KeyModifiers::COMMAND.with_shift(),
        )));
        assert_eq!(key, "`");
        assert!(flags.contains(NSEventModifierFlags::Command));
        assert!(flags.contains(NSEventModifierFlags::Shift));
    }
}
