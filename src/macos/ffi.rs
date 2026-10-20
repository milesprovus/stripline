//! Raw declarations for the C APIs Stripline uses that have no maintained
//! Rust bindings: Accessibility (HIServices), CGEventTap, a few private
//! SkyLight calls for Spaces, and the Text Input Sources keyboard-layout API.

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::c_void;

use core_foundation_sys::array::CFArrayRef;
use core_foundation_sys::base::{Boolean, CFTypeID, CFTypeRef};
use core_foundation_sys::dictionary::CFDictionaryRef;
use core_foundation_sys::mach_port::CFMachPortRef;
use core_foundation_sys::runloop::CFRunLoopSourceRef;
use core_foundation_sys::string::CFStringRef;
use core_foundation_sys::uuid::CFUUIDRef;

pub type Pid = i32;
pub type AXUIElementRef = *const c_void;
pub type AXObserverRef = *const c_void;
pub type AXValueRef = *const c_void;
pub type AXError = i32;

pub const AX_OK: AXError = 0;
pub const AX_ERR_INVALID_ELEMENT: AXError = -25202;
pub const AX_ERR_CANNOT_COMPLETE: AXError = -25204;
pub const AX_ERR_NOTIFICATION_ALREADY_REGISTERED: AXError = -25209;
pub const AX_ERR_API_DISABLED: AXError = -25211;

pub const AX_VALUE_CGPOINT: u32 = 1;
pub const AX_VALUE_CGSIZE: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

pub type AXObserverCallback = unsafe extern "C" fn(
    observer: AXObserverRef,
    element: AXUIElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub static kAXTrustedCheckOptionPrompt: CFStringRef;

    pub fn AXIsProcessTrusted() -> Boolean;
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;

    pub fn AXUIElementGetTypeID() -> CFTypeID;
    pub fn AXUIElementCreateApplication(pid: Pid) -> AXUIElementRef;
    pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    pub fn AXUIElementSetAttributeValue(element: AXUIElementRef, attribute: CFStringRef, value: CFTypeRef) -> AXError;
    pub fn AXUIElementIsAttributeSettable(
        element: AXUIElementRef,
        attribute: CFStringRef,
        settable: *mut Boolean,
    ) -> AXError;
    pub fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    pub fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut Pid) -> AXError;
    pub fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> AXError;
    pub fn AXUIElementCopyElementAtPosition(
        application: AXUIElementRef,
        x: f32,
        y: f32,
        element: *mut AXUIElementRef,
    ) -> AXError;

    pub fn AXValueGetTypeID() -> CFTypeID;
    pub fn AXValueCreate(value_type: u32, value: *const c_void) -> AXValueRef;
    pub fn AXValueGetValue(value: AXValueRef, value_type: u32, out: *mut c_void) -> Boolean;

    pub fn AXObserverCreate(pid: Pid, callback: AXObserverCallback, observer: *mut AXObserverRef) -> AXError;
    pub fn AXObserverAddNotification(
        observer: AXObserverRef,
        element: AXUIElementRef,
        notification: CFStringRef,
        refcon: *mut c_void,
    ) -> AXError;
    pub fn AXObserverRemoveNotification(
        observer: AXObserverRef,
        element: AXUIElementRef,
        notification: CFStringRef,
    ) -> AXError;
    pub fn AXObserverGetRunLoopSource(observer: AXObserverRef) -> CFRunLoopSourceRef;

    /// Private but long-standing: the CGWindowID behind an AX window element.
    pub fn _AXUIElementGetWindow(element: AXUIElementRef, window_id: *mut u32) -> AXError;

    pub fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> CFUUIDRef;
}

pub type CGEventRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;
pub type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

pub const TAP_SESSION: u32 = 1; // kCGSessionEventTap
pub const TAP_HEAD_INSERT: u32 = 0; // kCGHeadInsertEventTap
pub const TAP_OPTION_DEFAULT: u32 = 0; // active filter
pub const TAP_OPTION_LISTEN_ONLY: u32 = 1;

pub const EV_LEFT_MOUSE_DOWN: u32 = 1;
pub const EV_LEFT_MOUSE_UP: u32 = 2;
pub const EV_MOUSE_MOVED: u32 = 5;
pub const EV_KEY_DOWN: u32 = 10;
/// NSEventTypeGesture: raw trackpad touches.
pub const EV_GESTURE: u32 = 29;
pub const EV_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
pub const EV_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

pub const FIELD_KEYCODE: u32 = 9; // kCGKeyboardEventKeycode

pub const fn event_mask(t: u32) -> u64 {
    1u64 << t
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventGetFlags(event: CGEventRef) -> u64;
    pub fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub fn CGEventCreate(source: *const c_void) -> CGEventRef;
    pub fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
    pub fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
    pub fn CGMainDisplayID() -> u32;

    // Private SkyLight entry points, re-exported by CoreGraphics. Used by most
    // macOS window managers to find which Space a window is on.
    pub fn CGSMainConnectionID() -> i32;
    pub fn CGSCopyManagedDisplaySpaces(cid: i32) -> CFArrayRef;
    pub fn CGSCopySpacesForWindows(cid: i32, mask: i32, window_ids: CFArrayRef) -> CFArrayRef;
}

/// kCGSAllSpacesMask
pub const CGS_ALL_SPACES: i32 = 0x7;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    pub static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    pub fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    pub fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> *mut c_void;
    pub fn TISGetInputSourceProperty(source: *mut c_void, key: CFStringRef) -> *mut c_void;
    pub fn LMGetKbdType() -> u8;
    pub fn UCKeyTranslate(
        layout: *const c_void,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    ) -> i32;
}

pub const UC_KEY_ACTION_DISPLAY: u16 = 3;
pub const UC_KEY_TRANSLATE_NO_DEAD_KEYS: u32 = 1;
