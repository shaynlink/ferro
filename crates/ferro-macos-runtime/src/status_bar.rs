//! Native macOS status-bar (`NSStatusItem`) and popover host support for widget apps.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2_app_kit::{
    NSPopover, NSPopoverBehavior, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
    NSViewController,
};
use objc2_foundation::{MainThreadMarker, NSRectEdge, NSString};

/// Configuration for creating a macOS menu-bar status item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusBarConfig {
    /// Text displayed in the macOS menu bar button.
    pub title: String,
    /// Optional tooltip shown on hover.
    pub tooltip: Option<String>,
}

impl StatusBarConfig {
    /// Creates a status-bar configuration with the given label.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            tooltip: None,
        }
    }

    /// Sets a hover tooltip.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

/// Owns a live `NSStatusItem` and optional transient `NSPopover` on the main thread.
pub struct StatusBarController {
    item: Retained<NSStatusItem>,
    popover: Option<Retained<NSPopover>>,
}

impl StatusBarController {
    /// Installs a status item into the macOS system status bar.
    #[must_use]
    pub fn new(main_thread: MainThreadMarker, config: &StatusBarConfig) -> Self {
        let bar = NSStatusBar::systemStatusBar();
        let item = bar.statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(main_thread) {
            button.setTitle(&NSString::from_str(&config.title));
            if let Some(tooltip) = &config.tooltip {
                button.setToolTip(Some(&NSString::from_str(tooltip)));
            }
        }
        Self {
            item,
            popover: None,
        }
    }

    /// Updates the status-bar button title.
    pub fn set_title(&self, main_thread: MainThreadMarker, title: &str) {
        if let Some(button) = self.item.button(main_thread) {
            button.setTitle(&NSString::from_str(title));
        }
    }

    /// Binds a click action selector to the status item's button.
    ///
    /// # Safety
    ///
    /// `target` must remain alive while the status item can send `action`, and `action` must
    /// match a valid selector on `target`.
    pub unsafe fn set_action(
        &self,
        main_thread: MainThreadMarker,
        target: &AnyObject,
        action: Sel,
    ) {
        if let Some(button) = self.item.button(main_thread) {
            // SAFETY: Caller guarantees target lifetime and selector validity.
            unsafe {
                button.setTarget(Some(target));
                button.setAction(Some(action));
            }
        }
    }

    /// Attaches a popover view controller and toggles visibility relative to the status button.
    pub fn toggle_popover(&mut self, main_thread: MainThreadMarker, controller: &NSViewController) {
        let popover = self.popover.get_or_insert_with(|| {
            let pop = NSPopover::new(main_thread);
            pop.setBehavior(NSPopoverBehavior::Transient);
            pop
        });
        popover.setContentViewController(Some(controller));
        if popover.isShown() {
            // SAFETY: Invoked on AppKit main thread with valid popover target.
            unsafe { popover.performClose(None) };
        } else if let Some(button) = self.item.button(main_thread) {
            let bounds = button.bounds();
            popover.showRelativeToRect_ofView_preferredEdge(bounds, &button, NSRectEdge::MinY);
        }
    }
}
