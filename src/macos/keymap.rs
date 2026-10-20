//! Character -> virtual key code for the active keyboard layout.

use std::collections::HashMap;

use core_foundation_sys::base::{CFRelease, CFTypeRef};
use core_foundation_sys::data::{CFDataGetBytePtr, CFDataRef};

use super::ffi::*;

/// Map every character the current layout produces without modifiers to the
/// first key code that produces it. Empty if the layout has no Unicode data.
pub fn current_layout() -> HashMap<char, u16> {
    let mut map = HashMap::new();
    unsafe {
        let mut source = TISCopyCurrentKeyboardLayoutInputSource();
        let mut data = layout_data(source);
        if data.is_null() {
            release(source);
            source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
            data = layout_data(source);
        }
        if !data.is_null() {
            let layout = CFDataGetBytePtr(data as CFDataRef) as *const std::ffi::c_void;
            let kbd = LMGetKbdType() as u32;
            for code in 0u16..128 {
                let mut dead = 0u32;
                let mut buf = [0u16; 4];
                let mut len = 0usize;
                let status = UCKeyTranslate(
                    layout,
                    code,
                    UC_KEY_ACTION_DISPLAY,
                    0,
                    kbd,
                    UC_KEY_TRANSLATE_NO_DEAD_KEYS,
                    &mut dead,
                    buf.len(),
                    &mut len,
                    buf.as_mut_ptr(),
                );
                if status != 0 || len != 1 {
                    continue;
                }
                if let Some(c) = char::from_u32(buf[0] as u32).filter(|c| !c.is_control()) {
                    for lc in c.to_lowercase() {
                        map.entry(lc).or_insert(code);
                    }
                }
            }
        }
        release(source);
    }
    map
}

unsafe fn layout_data(source: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    if source.is_null() {
        return std::ptr::null_mut();
    }
    TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData)
}

unsafe fn release(source: *mut std::ffi::c_void) {
    if !source.is_null() {
        CFRelease(source as CFTypeRef);
    }
}
