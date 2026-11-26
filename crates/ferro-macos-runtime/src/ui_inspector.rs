//! Native `AppKit` UI hierarchy inspection and programmatic automation actions.

use ferro_sdk::WindowId;
use ferro_testkit::{UiAction, UiNode, UiRect, UiRole, UiSelector, UiTree};
use objc2::Message;
use objc2_app_kit::{
    NSAccessibility, NSButton, NSControl, NSScrollView, NSTextField, NSTextView, NSView, NSWindow,
};
use objc2_foundation::{NSSize, NSString};

/// Captures a live [`UiTree`] snapshot by walking `window` and its `AppKit` subviews.
#[must_use]
pub fn capture_ui_tree(app_name: &str, window_id: &WindowId, window: &NSWindow) -> UiTree {
    let frame = window.frame();
    let title = window.title().to_string();
    let mut root = UiNode::new(
        window.class().name().to_string_lossy().into_owned(),
        UiRole::Window,
        UiRect::new(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ),
    )
    .with_id(window_id.as_str())
    .with_title(title)
    .with_visible(window.isVisible());
    root.focused = window.isKeyWindow();

    if let Some(content) = window.contentView() {
        root.children.push(capture_view_node(&content));
    }

    UiTree {
        app_name: app_name.to_owned(),
        window_id: window_id.as_str().to_owned(),
        root,
    }
}

fn capture_view_node(view: &NSView) -> UiNode {
    let class_name = view.class().name().to_string_lossy().into_owned();
    let rect = view.frame();
    let frame = UiRect::new(
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    );

    let role = classify_view_role(view, &class_name);
    let mut node = UiNode::new(class_name, role, frame).with_visible(!view.isHidden());

    if let Some(ax_id) = view.accessibilityIdentifier() {
        let id = ax_id.to_string();
        if !id.is_empty() {
            node.id = Some(id);
        }
    }
    if let Some(label) = view.accessibilityLabel() {
        let text = label.to_string();
        if !text.is_empty() {
            node.title = Some(text);
        }
    }

    if let Some(button) = view.downcast_ref::<NSButton>() {
        let title = button.title().to_string();
        if !title.is_empty() {
            node.title = Some(title);
        }
        node.enabled = button.isEnabled();
    } else if let Some(field) = view.downcast_ref::<NSTextField>() {
        let text = field.stringValue().to_string();
        if !text.is_empty() {
            node.value = Some(text);
        }
        node.enabled = field.isEnabled();
    } else if let Some(control) = view.downcast_ref::<NSControl>() {
        node.enabled = control.isEnabled();
        let val = control.stringValue().to_string();
        if !val.is_empty() {
            node.value = Some(val);
        }
    } else if let Some(text_view) = view.downcast_ref::<NSTextView>() {
        let text = text_view.string().to_string();
        if !text.is_empty() {
            node.value = Some(text);
        }
    }

    for subview in view.subviews() {
        node.children.push(capture_view_node(&subview));
    }
    node
}

fn classify_view_role(view: &NSView, class_name: &str) -> UiRole {
    if view.downcast_ref::<NSButton>().is_some() {
        return UiRole::Button;
    }
    if let Some(field) = view.downcast_ref::<NSTextField>() {
        return if field.isEditable() {
            UiRole::TextField
        } else {
            UiRole::Label
        };
    }
    if view.downcast_ref::<NSTextView>().is_some() {
        return UiRole::TextField;
    }
    if view.downcast_ref::<NSScrollView>().is_some() {
        return UiRole::ScrollView;
    }
    if class_name.contains("WKWebView") {
        return UiRole::WebView;
    }
    if class_name.contains("NSTableView") || class_name.contains("NSOutlineView") {
        return UiRole::List;
    }
    UiRole::View
}

/// Executes a UI automation action against a live `NSWindow` on the `AppKit` main thread.
///
/// # Errors
///
/// Returns an error if the selector target cannot be resolved or the control does not support
/// the requested action.
pub fn execute_ui_action(
    app_name: &str,
    window_id: &WindowId,
    window: &NSWindow,
    action: &UiAction,
) -> Result<String, String> {
    match action {
        UiAction::Click { selector } => {
            let content = window
                .contentView()
                .ok_or_else(|| "window has no content view".to_owned())?;
            let target = find_matching_view(&content, selector)
                .ok_or_else(|| format!("UI element matching {selector:?} not found"))?;
            let control = target
                .downcast_ref::<NSControl>()
                .ok_or_else(|| "matched view is not an NSControl".to_owned())?;
            if !control.isEnabled() {
                return Err("matched control is disabled".to_owned());
            }
            // SAFETY: Invoked on AppKit's main thread on a valid enabled NSControl.
            unsafe { control.performClick(None) };
            Ok("clicked".to_owned())
        }
        UiAction::SetText { selector, text } => {
            let content = window
                .contentView()
                .ok_or_else(|| "window has no content view".to_owned())?;
            let target = find_matching_view(&content, selector)
                .ok_or_else(|| format!("UI element matching {selector:?} not found"))?;
            if let Some(control) = target.downcast_ref::<NSControl>() {
                control.setStringValue(&NSString::from_str(text));
                return Ok("updated control text".to_owned());
            }
            if let Some(text_view) = target.downcast_ref::<NSTextView>() {
                text_view.setString(&NSString::from_str(text));
                return Ok("updated text view".to_owned());
            }
            Err("matched view does not support SetText".to_owned())
        }
        UiAction::ResizeWindow { width, height, .. } => {
            window.setContentSize(NSSize::new(*width, *height));
            Ok(format!("resized to {width}x{height}"))
        }
        _ => {
            let mut tree = capture_ui_tree(app_name, window_id, window);
            let scenario = ferro_testkit::AutomationScenario {
                name: "inline-action".to_owned(),
                steps: vec![action.clone()],
            };
            let report = ferro_testkit::evaluate_scenario_on_tree(&scenario, &mut tree, |_, _| {
                Err("unsupported nested mutation".to_owned())
            });
            if report.passed {
                Ok("ok".to_owned())
            } else {
                Err(report
                    .steps
                    .first()
                    .and_then(|step| step.detail.clone())
                    .unwrap_or_else(|| "UI assertion failed".to_owned()))
            }
        }
    }
}

fn find_matching_view(view: &NSView, selector: &UiSelector) -> Option<objc2::rc::Retained<NSView>> {
    if view_matches(view, selector) {
        return Some(view.retain());
    }
    for subview in view.subviews() {
        if let Some(found) = find_matching_view(&subview, selector) {
            return Some(found);
        }
    }
    None
}

fn view_matches(view: &NSView, selector: &UiSelector) -> bool {
    match selector {
        UiSelector::Id(expected) => view
            .accessibilityIdentifier()
            .is_some_and(|id| id.to_string() == *expected),
        UiSelector::Text(needle) => {
            if let Some(label) = view.accessibilityLabel()
                && label.to_string().contains(needle)
            {
                return true;
            }
            if let Some(button) = view.downcast_ref::<NSButton>()
                && button.title().to_string().contains(needle)
            {
                return true;
            }
            if let Some(control) = view.downcast_ref::<NSControl>()
                && control.stringValue().to_string().contains(needle)
            {
                return true;
            }
            false
        }
    }
}
