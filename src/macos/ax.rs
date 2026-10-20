//! Safe wrapper around `AXUIElementRef`.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::{CFEqual, CFGetTypeID, CFRelease, CFRetain, CFTypeRef};

use super::ffi::*;
use crate::engine::Rect;

pub mod attr {
    pub const WINDOWS: &str = "AXWindows";
    pub const FOCUSED_WINDOW: &str = "AXFocusedWindow";
    pub const POSITION: &str = "AXPosition";
    pub const SIZE: &str = "AXSize";
    pub const TITLE: &str = "AXTitle";
    pub const ROLE: &str = "AXRole";
    pub const SUBROLE: &str = "AXSubrole";
    pub const MINIMIZED: &str = "AXMinimized";
    pub const MAIN: &str = "AXMain";
    pub const FRONTMOST: &str = "AXFrontmost";
    pub const FULL_SCREEN: &str = "AXFullScreen";
    pub const ZOOM_BUTTON: &str = "AXZoomButton";
    pub const ENABLED: &str = "AXEnabled";
    pub const WINDOW: &str = "AXWindow";
    pub const ENHANCED_UI: &str = "AXEnhancedUserInterface";
}

pub mod notif {
    pub const WINDOW_CREATED: &str = "AXWindowCreated";
    pub const FOCUSED_WINDOW_CHANGED: &str = "AXFocusedWindowChanged";
    pub const DESTROYED: &str = "AXUIElementDestroyed";
    pub const MINIATURIZED: &str = "AXWindowMiniaturized";
    pub const DEMINIATURIZED: &str = "AXWindowDeminiaturized";
    pub const MOVED: &str = "AXWindowMoved";
    pub const RESIZED: &str = "AXWindowResized";
    pub const APP_HIDDEN: &str = "AXApplicationHidden";
    pub const APP_SHOWN: &str = "AXApplicationShown";
}

pub const ROLE_WINDOW: &str = "AXWindow";
pub const SUBROLE_STANDARD: &str = "AXStandardWindow";
pub const ACTION_RAISE: &str = "AXRaise";

pub fn cfstr(s: &'static str) -> CFString {
    CFString::from_static_string(s)
}

/// An owned (retained) accessibility element.
pub struct AxEl(NonNull<c_void>);

impl AxEl {
    /// Take ownership of a +1 reference.
    ///
    /// # Safety
    /// `ptr` must be null or a valid, owned AXUIElementRef.
    pub unsafe fn from_create(ptr: AXUIElementRef) -> Option<AxEl> {
        NonNull::new(ptr as *mut c_void).map(AxEl)
    }

    /// Retain a borrowed (+0) reference.
    ///
    /// # Safety
    /// `ptr` must be null or a valid AXUIElementRef.
    pub unsafe fn from_get(ptr: AXUIElementRef) -> Option<AxEl> {
        let el = NonNull::new(ptr as *mut c_void)?;
        CFRetain(el.as_ptr() as CFTypeRef);
        Some(AxEl(el))
    }

    pub fn as_ptr(&self) -> AXUIElementRef {
        self.0.as_ptr() as AXUIElementRef
    }

    pub fn application(pid: Pid) -> Option<AxEl> {
        unsafe { Self::from_create(AXUIElementCreateApplication(pid)) }
    }

    pub fn system_wide() -> Option<AxEl> {
        unsafe { Self::from_create(AXUIElementCreateSystemWide()) }
    }

    fn copy(&self, name: &'static str) -> Result<CFType, AXError> {
        let mut out: CFTypeRef = ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(self.as_ptr(), cfstr(name).as_concrete_TypeRef(), &mut out) };
        if err != AX_OK {
            return Err(err);
        }
        if out.is_null() {
            return Err(AX_ERR_CANNOT_COMPLETE);
        }
        Ok(unsafe { CFType::wrap_under_create_rule(out) })
    }

    pub fn string(&self, name: &'static str) -> Option<String> {
        self.copy(name).ok()?.downcast::<CFString>().map(|s| s.to_string())
    }

    pub fn boolean(&self, name: &'static str) -> Option<bool> {
        self.copy(name).ok()?.downcast::<CFBoolean>().map(bool::from)
    }

    pub fn element(&self, name: &'static str) -> Option<AxEl> {
        let v = self.copy(name).ok()?;
        if v.type_of() != unsafe { AXUIElementGetTypeID() } {
            return None;
        }
        unsafe { Self::from_get(v.as_CFTypeRef()) }
    }

    pub fn elements(&self, name: &'static str) -> Vec<AxEl> {
        let Ok(v) = self.copy(name) else { return Vec::new() };
        let Some(arr) = v.downcast::<CFArray>() else { return Vec::new() };
        let raw = arr.as_concrete_TypeRef() as CFArrayRef;
        let ax_type = unsafe { AXUIElementGetTypeID() };
        let n = unsafe { CFArrayGetCount(raw) };
        (0..n)
            .filter_map(|i| unsafe {
                let item = CFArrayGetValueAtIndex(raw, i);
                if item.is_null() || CFGetTypeID(item) != ax_type {
                    return None;
                }
                Self::from_get(item)
            })
            .collect()
    }

    fn value<T: Default>(&self, name: &'static str, kind: u32) -> Option<T> {
        let v = self.copy(name).ok()?;
        if v.type_of() != unsafe { AXValueGetTypeID() } {
            return None;
        }
        let mut out = T::default();
        let ok = unsafe { AXValueGetValue(v.as_CFTypeRef(), kind, &mut out as *mut T as *mut c_void) };
        (ok != 0).then_some(out)
    }

    pub fn position(&self) -> Option<CGPoint> {
        self.value(attr::POSITION, AX_VALUE_CGPOINT)
    }

    pub fn size(&self) -> Option<CGSize> {
        self.value(attr::SIZE, AX_VALUE_CGSIZE)
    }

    pub fn frame(&self) -> Option<Rect> {
        let p = self.position()?;
        let s = self.size()?;
        Some(Rect::new(p.x, p.y, s.width, s.height))
    }

    fn set_value<T>(&self, name: &'static str, kind: u32, v: &T) -> AXError {
        unsafe {
            let value = AXValueCreate(kind, v as *const T as *const c_void);
            if value.is_null() {
                return AX_ERR_CANNOT_COMPLETE;
            }
            let err = AXUIElementSetAttributeValue(self.as_ptr(), cfstr(name).as_concrete_TypeRef(), value);
            CFRelease(value);
            err
        }
    }

    pub fn set_position(&self, x: f64, y: f64) -> AXError {
        self.set_value(attr::POSITION, AX_VALUE_CGPOINT, &CGPoint { x, y })
    }

    pub fn set_size(&self, w: f64, h: f64) -> AXError {
        self.set_value(attr::SIZE, AX_VALUE_CGSIZE, &CGSize { width: w, height: h })
    }

    pub fn set_bool(&self, name: &'static str, v: bool) -> AXError {
        let b = CFBoolean::from(v);
        unsafe { AXUIElementSetAttributeValue(self.as_ptr(), cfstr(name).as_concrete_TypeRef(), b.as_CFTypeRef()) }
    }

    pub fn perform(&self, action: &'static str) -> AXError {
        unsafe { AXUIElementPerformAction(self.as_ptr(), cfstr(action).as_concrete_TypeRef()) }
    }

    pub fn settable(&self, name: &'static str) -> bool {
        let mut out: u8 = 0;
        let err = unsafe { AXUIElementIsAttributeSettable(self.as_ptr(), cfstr(name).as_concrete_TypeRef(), &mut out) };
        err == AX_OK && out != 0
    }

    pub fn pid(&self) -> Option<Pid> {
        let mut pid = 0;
        (unsafe { AXUIElementGetPid(self.as_ptr(), &mut pid) } == AX_OK).then_some(pid)
    }

    pub fn window_id(&self) -> Option<u32> {
        let mut id = 0;
        let err = unsafe { _AXUIElementGetWindow(self.as_ptr(), &mut id) };
        (err == AX_OK && id != 0).then_some(id)
    }

    /// The window element under a screen point, if any (system-wide hit test).
    pub fn window_at(x: f64, y: f64) -> Option<AxEl> {
        let sys = Self::system_wide()?;
        let mut out: AXUIElementRef = ptr::null();
        let err = unsafe { AXUIElementCopyElementAtPosition(sys.as_ptr(), x as f32, y as f32, &mut out) };
        if err != AX_OK {
            return None;
        }
        let el = unsafe { Self::from_create(out) }?;
        if el.string(attr::ROLE).as_deref() == Some(ROLE_WINDOW) {
            return Some(el);
        }
        el.element(attr::WINDOW)
    }

    pub fn set_global_timeout(seconds: f32) {
        if let Some(sys) = Self::system_wide() {
            unsafe { AXUIElementSetMessagingTimeout(sys.as_ptr(), seconds) };
        }
    }
}

impl Clone for AxEl {
    fn clone(&self) -> Self {
        unsafe { CFRetain(self.0.as_ptr() as CFTypeRef) };
        AxEl(self.0)
    }
}

impl Drop for AxEl {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0.as_ptr() as CFTypeRef) };
    }
}

impl PartialEq for AxEl {
    fn eq(&self, other: &Self) -> bool {
        unsafe { CFEqual(self.as_ptr(), other.as_ptr()) != 0 }
    }
}

impl std::fmt::Debug for AxEl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AxEl({:p})", self.0)
    }
}

/// Is this process allowed to use the Accessibility API? Optionally shows
/// the system prompt that leads to System Settings.
pub fn is_trusted(prompt: bool) -> bool {
    use core_foundation::dictionary::CFDictionary;
    unsafe {
        if !prompt {
            return AXIsProcessTrusted() != 0;
        }
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let opts = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
        AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef()) != 0
    }
}
