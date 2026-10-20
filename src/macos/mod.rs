//! macOS front end: application shell, system notifications, and the
//! Accessibility-driven window manager.

mod ax;
mod border;
mod display;
pub mod doctor;
mod eventtap;
mod ffi;
mod keymap;
mod menu;
mod observer;
mod sink;
mod wm;

use std::ptr::NonNull;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDidChangeScreenParametersNotification,
    NSRunningApplication, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification, NSWorkspaceDidWakeNotification,
    NSWorkspaceScreensDidWakeNotification, NSWorkspaceSessionDidBecomeActiveNotification,
};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSDistributedNotificationCenter, NSNotification, NSNotificationCenter,
    NSNotificationName, NSObjectProtocol,
};

use crate::ipc;
use wm::SystemEvent;

pub fn run() -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("must be started on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    ipc::serve(|line| dispatch::Queue::main().exec_sync(move || wm::command(line))).map_err(|e| e.to_string())?;
    handle_signals();
    menu::install(mtm);
    observe_system();
    wm::install(wm::Wm::new(mtm));
    wait_for_accessibility(true);

    log::info!("stripline {} running", env!("CARGO_PKG_VERSION"));
    app.run();
    Ok(())
}

/// Start once the user has granted Accessibility access; ask once, then poll.
fn wait_for_accessibility(first: bool) {
    if ax::is_trusted(first) {
        wm::start();
        return;
    }
    if first {
        log::warn!(
            "waiting for Accessibility permission: System Settings > Privacy & Security > Accessibility, \
             then enable the entry for {}",
            std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "stripline".into())
        );
    }
    dispatch::Queue::main().exec_after(Duration::from_secs(2), || wait_for_accessibility(false));
}

fn handle_signals() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGINT, SIGTERM, SIGHUP]) else {
        log::warn!("could not install signal handlers");
        return;
    };
    std::thread::spawn(move || {
        for sig in signals.forever() {
            let action = if sig == SIGHUP { crate::action::Action::ReloadConfig } else { crate::action::Action::Quit };
            dispatch::Queue::main().exec_async(move || wm::run_action(action));
        }
    });
}

type Token = Retained<ProtocolObject<dyn NSObjectProtocol>>;

fn subscribe(center: &NSNotificationCenter, name: &NSNotificationName, f: impl Fn(&NSNotification) + 'static) -> Token {
    let block = RcBlock::new(move |n: NonNull<NSNotification>| {
        // SAFETY: the notification center passes a valid notification.
        f(unsafe { n.as_ref() })
    });
    // SAFETY: a nil queue runs the block on the posting thread, which is the
    // main thread for workspace and application notifications; the block
    // only forwards to the main queue.
    unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
}

fn pid_of(n: &NSNotification) -> Option<i32> {
    let info = n.userInfo()?;
    // SAFETY: NSWorkspaceApplicationKey is a valid static string.
    let key = unsafe { NSWorkspaceApplicationKey };
    let app = info.objectForKey(key)?;
    app.downcast::<NSRunningApplication>().ok().map(|a| a.processIdentifier())
}

fn post(ev: SystemEvent) {
    dispatch::Queue::main().exec_async(move || wm::system_event(ev));
}

fn observe_system() {
    let ws = NSWorkspace::sharedWorkspace().notificationCenter();
    let app_center = NSNotificationCenter::defaultCenter();
    let dist = NSDistributedNotificationCenter::defaultCenter();
    // SAFETY: these are valid static notification names.
    let tokens: Vec<Token> = unsafe {
        vec![
            subscribe(&ws, NSWorkspaceDidLaunchApplicationNotification, |n| {
                if let Some(p) = pid_of(n) {
                    post(SystemEvent::AppLaunched(p));
                }
            }),
            subscribe(&ws, NSWorkspaceDidTerminateApplicationNotification, |n| {
                if let Some(p) = pid_of(n) {
                    post(SystemEvent::AppTerminated(p));
                }
            }),
            subscribe(&ws, NSWorkspaceDidActivateApplicationNotification, |n| {
                if let Some(p) = pid_of(n) {
                    post(SystemEvent::AppActivated(p));
                }
            }),
            subscribe(&ws, NSWorkspaceActiveSpaceDidChangeNotification, |_| post(SystemEvent::SpaceChanged)),
            subscribe(&ws, NSWorkspaceDidWakeNotification, |_| post(SystemEvent::Woke)),
            subscribe(&ws, NSWorkspaceScreensDidWakeNotification, |_| post(SystemEvent::Woke)),
            subscribe(&ws, NSWorkspaceSessionDidBecomeActiveNotification, |_| post(SystemEvent::Woke)),
            subscribe(&app_center, NSApplicationDidChangeScreenParametersNotification, |_| {
                post(SystemEvent::ScreensChanged)
            }),
            subscribe(&dist, ns_string!("com.apple.Carbon.TISNotifySelectedKeyboardInputSourceChanged"), |_| {
                post(SystemEvent::KeyboardLayoutChanged)
            }),
        ]
    };
    // Observers live for the whole process.
    std::mem::forget(tokens);
}
