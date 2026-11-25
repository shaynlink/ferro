//! Native `AppKit` view, vibrancy, traffic-light layout, and Metal layer helpers.

use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindow, NSWindowButton,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect};
use objc2_quartz_core::CAMetalLayer;

/// Attaches a `CAMetalLayer` directly to `view` for hardware-accelerated rendering.
#[must_use]
pub fn attach_metal_layer(view: &NSView) -> Retained<CAMetalLayer> {
    view.setWantsLayer(true);
    let layer = CAMetalLayer::new();
    view.setLayer(Some(&layer));
    layer
}

/// Creates a translucent macOS `NSVisualEffectView` (e.g. for sidebars or floating islands).
#[must_use]
pub fn create_vibrancy_view(
    main_thread: MainThreadMarker,
    frame: NSRect,
    material: NSVisualEffectMaterial,
) -> Retained<NSVisualEffectView> {
    let view = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(main_thread), frame);
    view.setMaterial(material);
    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    view.setState(NSVisualEffectState::FollowsWindowActiveState);
    view
}

/// Adjusts the horizontal and vertical offsets of standard macOS traffic-light buttons.
pub fn configure_traffic_lights(window: &NSWindow, origin_x: f64, origin_y: f64) {
    let buttons = [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ];
    let mut cursor_x = origin_x;
    for kind in buttons {
        let Some(button) = window.standardWindowButton(kind) else {
            continue;
        };
        let frame = button.frame();
        button.setFrameOrigin(NSPoint::new(cursor_x, origin_y));
        cursor_x += frame.size.width + 6.0;
    }
}
