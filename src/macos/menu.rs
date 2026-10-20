//! Menu-bar status item.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{ns_string, MainThreadMarker, NSString};

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
    _item: Retained<NSStatusItem>,
    _target: Retained<MenuTarget>,
    toggle: Retained<NSMenuItem>,
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
    if let Some(button) = item.button(mtm) {
        match NSImage::imageWithSystemSymbolName_accessibilityDescription(
            ns_string!("rectangle.split.3x1"),
            Some(ns_string!("Stripline")),
        ) {
            Some(img) => button.setImage(Some(&img)),
            None => button.setTitle(ns_string!("▥")),
        }
    }
    let target = MenuTarget::new(mtm);
    let menu = NSMenu::new(mtm);
    let toggle = add(&menu, &target, "Pause Tiling", sel!(toggleTiling:), mtm);
    add(&menu, &target, "Re-tile All Windows", sel!(retile:), mtm);
    add(&menu, &target, "Open Config…", sel!(openConfig:), mtm);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add(&menu, &target, "Quit Stripline", sel!(quit:), mtm);
    item.setMenu(Some(&menu));
    MENU.with(|m| *m.borrow_mut() = Some(Menu { _item: item, _target: target, toggle }));
}

pub fn set_paused(paused: bool) {
    MENU.with(|m| {
        if let Some(m) = m.borrow().as_ref() {
            m.toggle.setTitle(&NSString::from_str(if paused { "Resume Tiling" } else { "Pause Tiling" }));
        }
    });
}
