//! The settings window: a native window hosting the settings page (served by
//! `crate::settings` on 127.0.0.1) in a WebKit view.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSEventModifierFlags, NSMenu, NSMenuItem, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{ns_string, MainThreadMarker, NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSURL};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

use crate::settings;

struct Settings {
    window: Retained<NSWindow>,
    web: Retained<WKWebView>,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// Show the settings window, creating it the first time. Reopening reloads
/// the page so it reflects the current config and displays.
pub fn open(mtm: MainThreadMarker) -> Result<String, String> {
    let url = settings::start()?;
    let app = NSApplication::sharedApplication(mtm);
    install_edit_menu(mtm, &app);
    SETTINGS.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some(s) = cell.as_ref() {
            // SAFETY: plain WebKit call on the main thread.
            unsafe { s.web.reload() };
            s.window.makeKeyAndOrderFront(None);
        } else {
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(940.0, 680.0));
            // SAFETY: standard window creation; the window is owned by SETTINGS.
            let window = unsafe {
                let w = NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    frame,
                    NSWindowStyleMask::Titled
                        | NSWindowStyleMask::Closable
                        | NSWindowStyleMask::Miniaturizable
                        | NSWindowStyleMask::Resizable,
                    NSBackingStoreType::Buffered,
                    false,
                );
                w.setReleasedWhenClosed(false);
                w
            };
            window.setTitle(ns_string!("Stripline Settings"));
            window.setContentMinSize(NSSize::new(640.0, 460.0));
            // SAFETY: WebKit views are created and used on the main thread.
            let web = unsafe {
                let config = WKWebViewConfiguration::new(mtm);
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config)
            };
            web.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            window.setContentView(Some(&web));
            let ns_url = NSURL::URLWithString(&NSString::from_str(&url)).ok_or("bad settings URL")?;
            // SAFETY: plain WebKit call on the main thread.
            unsafe { web.loadRequest(&NSURLRequest::requestWithURL(&ns_url)) };
            window.center();
            window.makeKeyAndOrderFront(None);
            *cell = Some(Settings { window, web });
        }
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        Ok("settings opened".into())
    })
}

fn item(mtm: MainThreadMarker, title: &str, action: objc2::runtime::Sel, key: &str) -> Retained<NSMenuItem> {
    // SAFETY: the actions are standard responder-chain selectors.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    }
}

/// Stripline has no menu bar of its own, but text fields need Edit menu key
/// equivalents for copy, paste and undo to work. The items have no target,
/// so they go to whatever has focus.
fn install_edit_menu(mtm: MainThreadMarker, app: &NSApplication) {
    if app.mainMenu().is_some() {
        return;
    }
    let main = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    app_menu.addItem(&item(mtm, "Close Window", sel!(performClose:), "w"));
    app_item.setSubmenu(Some(&app_menu));
    main.addItem(&app_item);

    let edit_item = NSMenuItem::new(mtm);
    let edit = NSMenu::new(mtm);
    edit.setTitle(ns_string!("Edit"));
    edit.addItem(&item(mtm, "Undo", sel!(undo:), "z"));
    let redo = item(mtm, "Redo", sel!(redo:), "z");
    redo.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    edit.addItem(&redo);
    edit.addItem(&NSMenuItem::separatorItem(mtm));
    edit.addItem(&item(mtm, "Cut", sel!(cut:), "x"));
    edit.addItem(&item(mtm, "Copy", sel!(copy:), "c"));
    edit.addItem(&item(mtm, "Paste", sel!(paste:), "v"));
    edit.addItem(&item(mtm, "Select All", sel!(selectAll:), "a"));
    edit_item.setSubmenu(Some(&edit));
    main.addItem(&edit_item);
    app.setMainMenu(Some(&main));
}
