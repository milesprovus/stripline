//! Displays, Spaces and the mouse.

use std::collections::HashMap;
use std::ptr;

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFRelease, CFTypeRef};
use core_foundation_sys::uuid::CFUUIDCreateString;
use objc2_app_kit::NSScreen;
use objc2_foundation::{ns_string, MainThreadMarker, NSNumber};

use super::ffi::*;
use crate::engine::Rect;

#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub id: u32,
    pub uuid: String,
    pub name: String,
    /// Full frame, global top-left coordinates.
    pub frame: Rect,
    /// Frame minus menu bar and Dock.
    pub visible: Rect,
    /// Space currently shown on this display (0 if unknown).
    pub space: u64,
}

pub fn display_uuid(id: u32) -> String {
    unsafe {
        let uuid = CGDisplayCreateUUIDFromDisplayID(id);
        if uuid.is_null() {
            return format!("display-{id}");
        }
        let s = CFUUIDCreateString(ptr::null(), uuid);
        CFRelease(uuid as CFTypeRef);
        if s.is_null() {
            return format!("display-{id}");
        }
        CFString::wrap_under_create_rule(s).to_string()
    }
}

/// All active displays, primary (menu bar) display first.
pub fn displays(mtm: MainThreadMarker) -> Vec<Display> {
    let screens = NSScreen::screens(mtm);
    // Cocoa puts the origin at the bottom-left of the primary screen; flip to
    // the top-left convention used by Accessibility and CoreGraphics.
    let primary_h = screens.firstObject().map(|s| s.frame().size.height).unwrap_or(0.0);
    let flip = |r: objc2_foundation::NSRect| {
        Rect::new(r.origin.x, primary_h - (r.origin.y + r.size.height), r.size.width, r.size.height)
    };
    screens
        .iter()
        .map(|screen| {
            let desc = screen.deviceDescription();
            let id = desc
                .objectForKey(ns_string!("NSScreenNumber"))
                .and_then(|n| n.downcast::<NSNumber>().ok())
                .map(|n| n.unsignedIntValue())
                .unwrap_or(0);
            Display {
                id,
                uuid: display_uuid(id),
                name: screen.localizedName().to_string(),
                frame: flip(screen.frame()),
                visible: flip(screen.visibleFrame()),
                space: 0,
            }
        })
        .collect()
}

/// Which Space each display shows, and which display owns each Space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spaces {
    /// display UUID -> current space id
    pub current: HashMap<String, u64>,
    /// space id -> display UUID
    pub owner: HashMap<u64, String>,
}

fn dict_get(dict: &CFDictionary<CFString, CFType>, key: &'static str) -> Option<CFType> {
    dict.find(CFString::from_static_string(key)).map(|v| v.clone())
}

fn space_id(dict: &CFDictionary<CFString, CFType>) -> Option<u64> {
    dict_get(dict, "id64")
        .or_else(|| dict_get(dict, "ManagedSpaceID"))
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_i64())
        .map(|n| n as u64)
}

/// Read the Space layout from SkyLight. `main_uuid` is used when "Displays
/// have separate Spaces" is off and macOS reports a single "Main" entry.
pub fn spaces(main_uuid: &str) -> Spaces {
    let mut out = Spaces::default();
    unsafe {
        let raw = CGSCopyManagedDisplaySpaces(CGSMainConnectionID());
        if raw.is_null() {
            return out;
        }
        let list: CFArray<CFType> = CFArray::wrap_under_create_rule(raw);
        for item in list.iter() {
            let Some(d) = item.downcast::<CFDictionary>() else { continue };
            let d: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_get_rule(d.as_concrete_TypeRef());
            let mut uuid = dict_get(&d, "Display Identifier")
                .and_then(|v| v.downcast::<CFString>())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if uuid.is_empty() || uuid == "Main" {
                uuid = main_uuid.to_string();
            }
            if let Some(cur) = dict_get(&d, "Current Space").and_then(|v| v.downcast::<CFDictionary>()) {
                let cur: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_get_rule(cur.as_concrete_TypeRef());
                if let Some(id) = space_id(&cur) {
                    out.current.insert(uuid.clone(), id);
                }
            }
            if let Some(list) = dict_get(&d, "Spaces").and_then(|v| v.downcast::<CFArray>()) {
                let list: CFArray<CFType> = CFArray::wrap_under_get_rule(list.as_concrete_TypeRef());
                for s in list.iter() {
                    let Some(s) = s.downcast::<CFDictionary>() else { continue };
                    let s: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_get_rule(s.as_concrete_TypeRef());
                    if let Some(id) = space_id(&s) {
                        out.owner.insert(id, uuid.clone());
                    }
                }
            }
        }
    }
    out
}

/// Spaces a window is on (usually exactly one).
pub fn spaces_for_window(wid: u32) -> Vec<u64> {
    unsafe {
        let ids = CFArray::from_CFTypes(&[CFNumber::from(wid as i32)]);
        let raw = CGSCopySpacesForWindows(CGSMainConnectionID(), CGS_ALL_SPACES, ids.as_concrete_TypeRef());
        if raw.is_null() {
            return Vec::new();
        }
        let list: CFArray<CFType> = CFArray::wrap_under_create_rule(raw);
        list.iter().filter_map(|v| v.downcast::<CFNumber>().and_then(|n| n.to_i64())).map(|n| n as u64).collect()
    }
}

pub fn mouse_location() -> (f64, f64) {
    unsafe {
        let ev = CGEventCreate(ptr::null());
        if ev.is_null() {
            return (0.0, 0.0);
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);
        (p.x, p.y)
    }
}

pub fn warp_mouse(x: f64, y: f64) {
    unsafe {
        CGWarpMouseCursorPosition(CGPoint { x, y });
    }
}

pub fn left_button_down() -> bool {
    // kCGEventSourceStateCombinedSessionState, kCGMouseButtonLeft
    unsafe { CGEventSourceButtonState(0, 0) }
}
