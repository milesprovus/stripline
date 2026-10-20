//! Menu-bar status item.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_app_kit::{
    NSCellImagePosition, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{ns_string, MainThreadMarker, NSDate, NSDateFormatter, NSDateFormatterStyle, NSString};

use super::wm;
use crate::action::Action;
use crate::config;

define_class!(
    // SAFETY: NSObject has no subclassing requirements and MenuTarget has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "StriplineMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(toggleTiling:))]
        fn toggle_tiling(&self, _sender: Option<&AnyObject>) {
            wm::run_action(Action::ToggleTiling);
        }

        #[unsafe(method(reloadConfig:))]
        fn reload_config(&self, _sender: Option<&AnyObject>) {
            wm::run_action(Action::ReloadConfig);
        }

        #[unsafe(method(retile:))]
        fn retile(&self, _sender: Option<&AnyObject>) {
            wm::run_action(Action::Retile);
        }

        #[unsafe(method(openConfig:))]
        fn open_config(&self, _sender: Option<&AnyObject>) {
            let path = config::config_path();
            if !path.exists() {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&path, config::EXAMPLE);
            }
            let _ = std::process::Command::new("open").arg("-t").arg(&path).spawn();
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            if let Err(e) = super::settings_window::open(self.mtm()) {
                log::error!("settings: {e}");
            }
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            wm::run_action(Action::Quit);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's init has this signature.
        unsafe { msg_send![super(this), init] }
    }
}

struct Menu {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    /// False when the SF Symbol was unavailable and the title is the icon.
    has_image: bool,
    _target: Retained<MenuTarget>,
    toggle: Retained<NSMenuItem>,
    /// Disabled line at the top showing the result of the last config load.
    status: Retained<NSMenuItem>,
}

thread_local! {
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

fn add(
    menu: &NSMenu,
    target: &MenuTarget,
    title: &str,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // SAFETY: the selector names a method defined on MenuTarget above.
    let item = unsafe {
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            ns_string!(""),
        );
        item.setTarget(Some(target));
        item
    };
    menu.addItem(&item);
    item
}

pub fn install(mtm: MainThreadMarker) {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let mut has_image = false;
    if let Some(button) = item.button(mtm) {
        match NSImage::imageWithSystemSymbolName_accessibilityDescription(
            ns_string!("rectangle.split.3x1"),
            Some(ns_string!("Stripline")),
        ) {
            Some(img) => {
                button.setImage(Some(&img));
                button.setImagePosition(NSCellImagePosition::ImageLeft);
                has_image = true;
            }
            None => button.setTitle(ns_string!("▥")),
        }
    }
    let target = MenuTarget::new(mtm);
    let menu = NSMenu::new(mtm);
    // No action, so AppKit shows it greyed out: a status line, not a button.
    let status = NSMenuItem::new(mtm);
    status.setTitle(ns_string!("Config not loaded yet"));
    menu.addItem(&status);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let settings = add(&menu, &target, "Settings…", sel!(openSettings:), mtm);
    settings.setKeyEquivalent(ns_string!(","));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let toggle = add(&menu, &target, "Pause Tiling", sel!(toggleTiling:), mtm);
    add(&menu, &target, "Reload Config", sel!(reloadConfig:), mtm);
    add(&menu, &target, "Re-tile All Windows", sel!(retile:), mtm);
    add(&menu, &target, "Open Config File…", sel!(openConfig:), mtm);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add(&menu, &target, "Quit Stripline", sel!(quit:), mtm);
    item.setMenu(Some(&menu));
    MENU.with(|m| *m.borrow_mut() = Some(Menu { mtm, item, has_image, _target: target, toggle, status }));
}

pub fn set_paused(paused: bool) {
    MENU.with(|m| {
        if let Some(m) = m.borrow().as_ref() {
            m.toggle.setTitle(&NSString::from_str(if paused { "Resume Tiling" } else { "Pause Tiling" }));
        }
    });
}

/// Show the outcome of the last config (re)load: `Ok(skipped bindings)` or
/// the error. The full error is in the item's tooltip and the log.
pub fn set_config_status(result: Result<usize, &str>) {
    let time = NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(
        &NSDate::now(),
        NSDateFormatterStyle::NoStyle,
        NSDateFormatterStyle::MediumStyle,
    );
    let (title, tip) = match result {
        Ok(0) => (format!("Config loaded at {time}"), None),
        Ok(n) => (format!("Config loaded at {time}, {n} bindings skipped"), None),
        Err(e) => {
            let first = e.lines().next().unwrap_or(e);
            let short: String = first.chars().take(60).collect();
            let ellipsis = if short.len() < first.len() { "…" } else { "" };
            (format!("⚠ Config error at {time}: {short}{ellipsis}"), Some(e.to_string()))
        }
    };
    MENU.with(|m| {
        if let Some(m) = m.borrow().as_ref() {
            m.status.setTitle(&NSString::from_str(&title));
            m.status.setToolTip(tip.as_deref().map(NSString::from_str).as_deref());
        }
    });
}

/// Show the current workspace number next to the menu bar icon.
pub fn set_workspace(n: u8) {
    MENU.with(|m| {
        if let Some(m) = m.borrow().as_ref() {
            if let Some(button) = m.item.button(m.mtm) {
                let title = if m.has_image { format!(" {n}") } else { format!("▥ {n}") };
                button.setTitle(&NSString::from_str(&title));
            }
        }
    });
}
