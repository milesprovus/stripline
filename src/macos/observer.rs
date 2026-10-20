//! Per-application AXObserver, delivering notifications on the main run loop.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use core_foundation::base::TCFType;
use core_foundation_sys::base::{CFRelease, CFTypeRef};
use core_foundation_sys::runloop::{
    kCFRunLoopCommonModes, CFRunLoopAddSource, CFRunLoopGetMain, CFRunLoopRemoveSource,
};

use super::ax::{cfstr, AxEl};
use super::ffi::*;

pub struct AppObserver {
    observer: NonNull<c_void>,
}

impl AppObserver {
    pub fn new(pid: Pid, callback: AXObserverCallback) -> Result<AppObserver, AXError> {
        let mut obs: AXObserverRef = ptr::null();
        let err = unsafe { AXObserverCreate(pid, callback, &mut obs) };
        if err != AX_OK {
            return Err(err);
        }
        let observer = NonNull::new(obs as *mut c_void).ok_or(AX_ERR_CANNOT_COMPLETE)?;
        unsafe {
            let source = AXObserverGetRunLoopSource(obs);
            CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        }
        Ok(AppObserver { observer })
    }

    fn raw(&self) -> AXObserverRef {
        self.observer.as_ptr() as AXObserverRef
    }

    /// Subscribe to `notification` on `element`. `refcon` comes back in the
    /// callback (Stripline passes the window id, or 0 for app-level events).
    pub fn add(&self, element: &AxEl, notification: &'static str, refcon: usize) -> AXError {
        let err = unsafe {
            AXObserverAddNotification(
                self.raw(),
                element.as_ptr(),
                cfstr(notification).as_concrete_TypeRef(),
                refcon as *mut c_void,
            )
        };
        if err == AX_ERR_NOTIFICATION_ALREADY_REGISTERED {
            AX_OK
        } else {
            err
        }
    }
}

impl Drop for AppObserver {
    fn drop(&mut self) {
        unsafe {
            let source = AXObserverGetRunLoopSource(self.raw());
            CFRunLoopRemoveSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
            CFRelease(self.observer.as_ptr() as CFTypeRef);
        }
    }
}
