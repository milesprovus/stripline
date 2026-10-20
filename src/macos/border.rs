//! Focus border: a transparent, click-through window holding one ring-shaped
//! Core Animation layer, kept just above the focused window.

use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowOrderingMode,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_quartz_core::{CALayer, CATransaction};

use crate::config::BorderConfig;
use crate::engine::{Rect, WinId};

pub struct Border {
    window: Retained<NSWindow>,
    ring: Retained<CALayer>,
    style: BorderConfig,
    /// Window and frame the border was last placed around.
    shown: Option<(WinId, Rect)>,
}

fn ns_rect(r: Rect) -> NSRect {
    NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.w, r.h))
}

impl Border {
    pub fn new(mtm: MainThreadMarker, style: &BorderConfig) -> Border {
        // SAFETY: standard window creation; released-when-closed is turned off
        // because the window is owned by this struct, not a controller.
        let window = unsafe {
            let w = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                ns_rect(Rect::new(0.0, 0.0, 10.0, 10.0)),
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            );
            w.setReleasedWhenClosed(false);
            w
        };
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        // Follow the user across Spaces, stay out of Mission Control and Cmd-`.
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::MoveToActiveSpace
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );

        let ring = CALayer::new();
        if let Some(view) = window.contentView() {
            view.setWantsLayer(true);
            if let Some(root) = view.layer() {
                root.setMasksToBounds(false);
                root.addSublayer(&ring);
            }
        }
        let mut border = Border { window, ring, style: style.clone(), shown: None };
        border.set_style(style);
        border
    }

    pub fn set_style(&mut self, style: &BorderConfig) {
        self.style = style.clone();
        let [r, g, b, a] = style.rgba();
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a).CGColor();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.ring.setBorderColor(Some(&color));
        self.ring.setBorderWidth(style.width);
        if style.glow > 0.0 {
            self.ring.setShadowColor(Some(&color));
            self.ring.setShadowOpacity(0.9);
            self.ring.setShadowRadius(style.glow / 2.0);
            self.ring.setShadowOffset(NSSize::new(0.0, 0.0));
        } else {
            self.ring.setShadowOpacity(0.0);
        }
        CATransaction::commit();
        // Force the next show() to re-place the ring with the new sizes.
        self.shown = None;
    }

    /// Place the border around `frame` (global, top-left origin) and order it
    /// directly above window `id`. `primary_height` converts to the
    /// bottom-left origin AppKit uses.
    pub fn show(&mut self, id: WinId, frame: Rect, primary_height: f64) {
        if self.shown.is_some_and(|(sid, f)| sid == id && f.approx_eq(&frame, 0.25)) {
            return;
        }
        let layout = self.style.layout(frame);
        let o = layout.outer;
        let cocoa = Rect::new(o.x, primary_height - (o.y + o.h), o.w, o.h);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.window.setFrame_display(ns_rect(cocoa), false);
        self.ring.setFrame(ns_rect(layout.ring));
        self.ring.setCornerRadius(layout.corner_radius);
        CATransaction::commit();
        if self.shown.is_none_or(|(sid, _)| sid != id) || !self.window.isVisible() {
            // Window numbers are global, so this works for other apps' windows.
            self.window.orderWindow_relativeTo(NSWindowOrderingMode::Above, id as isize);
        }
        self.shown = Some((id, frame));
    }

    pub fn hide(&mut self) {
        if self.shown.take().is_some() {
            self.window.orderOut(None);
        }
    }
}
