//! Global input: an active tap that swallows bound key presses, and a
//! listen-only tap for mouse buttons, mouse movement and trackpad touches.
//!
//! Tap callbacks run on the main thread inside the event pipeline, so they do
//! the minimum and hand everything else to the main queue.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};

use core_foundation_sys::mach_port::{CFMachPortCreateRunLoopSource, CFMachPortRef};
use core_foundation_sys::runloop::{kCFRunLoopCommonModes, CFRunLoopAddSource, CFRunLoopGetMain};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSEvent, NSEventType, NSTouchPhase};

use super::ffi::*;
use super::wm;
use crate::action::Action;
use crate::keys::Mods;

thread_local! {
    static BINDINGS: RefCell<HashMap<(u16, Mods), Action>> = RefCell::new(HashMap::new());
    static PAUSED: Cell<bool> = const { Cell::new(false) };
    static TRACK_MOUSE_MOVES: Cell<bool> = const { Cell::new(false) };
    static LAST_MOVE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// While set (and not expired), every key reaches the app: the settings
    /// window is recording a new shortcut.
    static RECORDING_UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
}

static KEY_TAP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static MOUSE_TAP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub fn set_bindings(map: HashMap<(u16, Mods), Action>) {
    BINDINGS.with(|b| *b.borrow_mut() = map);
}

/// Let all keys through while a shortcut is being recorded. Expires on its
/// own after 15 seconds in case the settings window never says it is done.
pub fn set_recording(on: bool) {
    RECORDING_UNTIL.with(|r| r.set(on.then(|| Instant::now() + Duration::from_secs(15))));
}

fn recording() -> bool {
    RECORDING_UNTIL.with(|r| r.get().is_some_and(|t| Instant::now() < t))
}

pub fn set_paused(paused: bool) {
    PAUSED.with(|p| p.set(paused));
}

pub fn set_track_mouse_moves(on: bool) {
    TRACK_MOUSE_MOVES.with(|t| t.set(on));
}

fn reenable(tap: &AtomicPtr<c_void>) {
    let port = tap.load(Ordering::Relaxed);
    if !port.is_null() {
        unsafe { CGEventTapEnable(port as CFMachPortRef, true) };
    }
}

unsafe extern "C" fn key_callback(
    _proxy: CGEventTapProxy,
    ty: u32,
    event: CGEventRef,
    _info: *mut c_void,
) -> CGEventRef {
    if ty == EV_TAP_DISABLED_BY_TIMEOUT || ty == EV_TAP_DISABLED_BY_USER_INPUT {
        reenable(&KEY_TAP);
        return event;
    }
    if ty != EV_KEY_DOWN {
        return event;
    }
    if recording() {
        return event;
    }
    let code = CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16;
    let mods = Mods::from_cg_flags(CGEventGetFlags(event));
    let action = BINDINGS.with(|b| b.borrow().get(&(code, mods)).copied());
    match action {
        Some(a) if !PAUSED.with(Cell::get) || a.works_when_paused() => {
            dispatch::Queue::main().exec_async(move || wm::run_action(a));
            ptr::null_mut()
        }
        _ => event,
    }
}

unsafe extern "C" fn mouse_callback(
    _proxy: CGEventTapProxy,
    ty: u32,
    event: CGEventRef,
    _info: *mut c_void,
) -> CGEventRef {
    match ty {
        EV_TAP_DISABLED_BY_TIMEOUT | EV_TAP_DISABLED_BY_USER_INPUT => reenable(&MOUSE_TAP),
        EV_LEFT_MOUSE_UP => {
            let p = CGEventGetLocation(event);
            dispatch::Queue::main().exec_async(move || wm::mouse_up(p.x, p.y));
        }
        EV_MOUSE_MOVED if TRACK_MOUSE_MOVES.with(Cell::get) => {
            let now = Instant::now();
            let due = LAST_MOVE.with(|l| l.get().is_none_or(|t| now - t > Duration::from_millis(40)));
            if due {
                LAST_MOVE.with(|l| l.set(Some(now)));
                let p = CGEventGetLocation(event);
                dispatch::Queue::main().exec_async(move || wm::mouse_moved(p.x, p.y));
            }
        }
        EV_GESTURE => {
            if let Some((n, x, y)) = touches(event) {
                dispatch::Queue::main().exec_async(move || wm::gesture(n, x, y));
            }
        }
        _ => {}
    }
    event
}

/// Count the fingers resting on the trackpad and their mean normalized
/// position (0..1 across the pad).
unsafe fn touches(event: CGEventRef) -> Option<(usize, f64, f64)> {
    autoreleasepool(|_| {
        let cg = &*(event as *const objc2_core_graphics::CGEvent);
        let ns = NSEvent::eventWithCGEvent(cg)?;
        if ns.r#type() != NSEventType::Gesture {
            return None;
        }
        let (mut n, mut sx, mut sy) = (0usize, 0.0, 0.0);
        for t in ns.allTouches().iter() {
            if t.phase().intersects(NSTouchPhase::Touching) && !t.isResting() {
                let p = t.normalizedPosition();
                n += 1;
                sx += p.x;
                sy += p.y;
            }
        }
        Some(if n == 0 { (0, 0.0, 0.0) } else { (n, sx / n as f64, sy / n as f64) })
    })
}

unsafe fn create(mask: u64, options: u32, cb: CGEventTapCallBack, slot: &AtomicPtr<c_void>) -> Result<(), String> {
    let port = CGEventTapCreate(TAP_SESSION, TAP_HEAD_INSERT, options, mask, cb, ptr::null_mut());
    if port.is_null() {
        return Err("could not create event tap (is Accessibility permission granted?)".into());
    }
    let source = CFMachPortCreateRunLoopSource(ptr::null(), port, 0);
    CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
    CGEventTapEnable(port, true);
    slot.store(port as *mut c_void, Ordering::Relaxed);
    Ok(())
}

pub fn install() -> Result<(), String> {
    unsafe {
        if KEY_TAP.load(Ordering::Relaxed).is_null() {
            create(event_mask(EV_KEY_DOWN), TAP_OPTION_DEFAULT, key_callback, &KEY_TAP)?;
        }
        if MOUSE_TAP.load(Ordering::Relaxed).is_null() {
            let mask = event_mask(EV_LEFT_MOUSE_UP) | event_mask(EV_MOUSE_MOVED) | event_mask(EV_GESTURE);
            create(mask, TAP_OPTION_LISTEN_ONLY, mouse_callback, &MOUSE_TAP)?;
        }
    }
    Ok(())
}
