//! An invisible window that takes keyboard focus when the user switches to an
//! empty workspace, so typing does not land in a window that was just hidden.

use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use crate::engine::Rect;

pub struct FocusSink {
    window: Retained<NSWindow>,
}

impl FocusSink {
    pub fn new(mtm: MainThreadMarker) -> FocusSink {
        // Titled, because borderless windows cannot become key.
        // SAFETY: standard window creation; the window is owned by this struct.
        let window = unsafe {
            let w = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            );
            w.setReleasedWhenClosed(false);
            w
        };
        window.setAlphaValue(0.0);
        window.setIgnoresMouseEvents(true);
        window.setHasShadow(false);
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::MoveToActiveSpace
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        FocusSink { window }
    }

    /// Activate Stripline with this window as key, on the given display
    /// (`visible` in global top-left coordinates).
    pub fn take_focus(&self, mtm: MainThreadMarker, visible: Rect, primary_height: f64) {
        let (cx, cy) = visible.center();
        self.window.setFrameOrigin(NSPoint::new(cx, primary_height - cy));
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        self.window.makeKeyAndOrderFront(None);
    }
}
