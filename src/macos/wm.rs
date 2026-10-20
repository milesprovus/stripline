//! The window manager: keeps the engine's strips in sync with real windows
//! and pushes computed frames back through the Accessibility API.
//!
//! Everything runs on the main thread. OS callbacks (AX observers, event taps,
//! workspace notifications, timers) reach the single `Wm` instance through
//! [`with`], which hands out a mutable borrow of the thread-local state.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::time::{Duration, Instant, SystemTime};

use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::string::CFStringRef;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSApplicationActivationPolicy, NSRunningApplication, NSScreen, NSWorkspace,
};
use objc2_foundation::MainThreadMarker;
use serde_json::json;

use super::ax::{self, attr, notif, AxEl};
use super::display::{self, Display, Spaces};
use super::eventtap;
use super::ffi::{AXObserverRef, AXUIElementRef, Pid, AX_ERR_CANNOT_COMPLETE, AX_ERR_INVALID_ELEMENT, AX_OK};
use super::keymap;
use super::menu;
use super::observer::AppObserver;
use crate::action::Action;
use crate::config::{self, Config, RuleEffect, Rules};
use crate::engine::session::{LiveWindow, Session};
use crate::engine::strip::{MAX_WIDTH, MIN_WIDTH};
use crate::engine::{place, Blocked, Geometry, LayoutParams, OpenAt, Rect, Side, Spring, Strip, StripKey, WinId};
use crate::ipc;
use crate::keys::Mods;

/// Pixels of a parked window left on screen (macOS keeps at least some of
/// every window visible).
const PARK_SLIVER: f64 = 1.0;
/// Trackpad travel (fraction of pad width) to screen-width multiplier.
const SWIPE_GAIN: f64 = 1.6;
/// How far ahead (seconds) swipe velocity is projected when snapping.
const SWIPE_PROJECTION: f64 = 0.15;
const HOUSEKEEPING: Duration = Duration::from_secs(2);

thread_local! {
    static WM: RefCell<Option<Wm>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Wm) -> R) -> Option<R> {
    WM.with(|cell| match cell.try_borrow_mut() {
        Ok(mut wm) => wm.as_mut().map(f),
        Err(_) => {
            log::warn!("re-entrant window manager call dropped");
            None
        }
    })
}

fn later(delay: Duration, f: impl FnOnce(&mut Wm) + Send + 'static) {
    dispatch::Queue::main().exec_after(delay, move || {
        with(f);
    });
}

// ---- entry points for callbacks -------------------------------------------

pub fn install(wm: Wm) {
    WM.with(|cell| *cell.borrow_mut() = Some(wm));
}

pub fn start() {
    with(Wm::start);
}

pub fn run_action(a: Action) {
    with(|wm| {
        if let Err(e) = wm.action(a) {
            log::debug!("{a}: {e}");
        }
    });
}

pub fn command(line: String) -> String {
    with(|wm| wm.command(&line)).unwrap_or_else(|| "error: busy, try again".into())
}

pub fn mouse_up(x: f64, y: f64) {
    with(|wm| wm.on_mouse_up(x, y));
}

pub fn mouse_moved(x: f64, y: f64) {
    with(|wm| wm.on_mouse_moved(x, y));
}

pub fn gesture(fingers: usize, x: f64, y: f64) {
    with(|wm| wm.on_gesture(fingers, x, y));
}

/// Events forwarded from NSWorkspace / NSNotificationCenter observers.
#[derive(Clone, Copy, Debug)]
pub enum SystemEvent {
    AppLaunched(Pid),
    AppTerminated(Pid),
    AppActivated(Pid),
    SpaceChanged,
    ScreensChanged,
    Woke,
    KeyboardLayoutChanged,
}

pub fn system_event(ev: SystemEvent) {
    with(|wm| wm.on_system_event(ev));
}

/// AXObserver callback. `refcon` is the window id for window-level
/// notifications and 0 for application-level ones.
pub unsafe extern "C" fn ax_callback(
    _observer: AXObserverRef,
    element: AXUIElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
) {
    let name = CFString::wrap_under_get_rule(notification).to_string();
    let Some(el) = AxEl::from_get(element) else { return };
    with(|wm| wm.on_ax(&name, el, refcon as usize));
}

// ---- state ----------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum WinState {
    Tiled(StripKey),
    Floating(Float),
    Minimized,
    /// The app is hidden (Cmd-H); remember where the window was.
    Hidden(StripKey),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Float {
    /// Dialogs, panels, fixed-size windows.
    Auto,
    Rule,
    User,
    FullScreen,
    /// On a display that Stripline leaves alone.
    Display,
}

struct Win {
    pid: Pid,
    el: AxEl,
    state: WinState,
}

struct App {
    el: AxEl,
    _observer: AppObserver,
    bundle: String,
    name: String,
    /// Set by assistive tech (and some apps); makes the app animate every
    /// frame change, so it is switched off while Stripline moves windows.
    enhanced_ui: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Move,
    Resize,
}

#[derive(Default)]
struct Gesture {
    key: Option<StripKey>,
    start: (f64, f64),
    last_x: f64,
    last_t: Option<Instant>,
    velocity: f64,
    horizontal: bool,
    ignore: bool,
}

enum Track {
    Skip,
    Retry,
    Registered(WinId, Option<(StripKey, RuleEffect)>),
}

pub struct Wm {
    mtm: MainThreadMarker,
    cfg: Config,
    cfg_stamp: Option<SystemTime>,
    rules: Rules,
    displays: Vec<Display>,
    spaces: Spaces,
    strips: HashMap<StripKey, Strip>,
    windows: HashMap<WinId, Win>,
    apps: HashMap<Pid, App>,
    started: bool,
    enabled: bool,
    own_pid: Pid,
    /// Last frame Stripline set for each window.
    applied: HashMap<WinId, Rect>,
    parked: HashSet<WinId>,
    /// Widths that apps refused to go below.
    min_widths: HashMap<WinId, f64>,
    anims: HashMap<StripKey, (Spring, Instant)>,
    dirty: HashSet<StripKey>,
    frame_scheduled: bool,
    drags: HashMap<WinId, Drag>,
    gesture: Gesture,
    ffm_target: Option<WinId>,
    retry: Vec<(AxEl, Pid, u8)>,
    session_dirty: bool,
}

impl Wm {
    pub fn new(mtm: MainThreadMarker) -> Wm {
        let path = config::config_path();
        let cfg = Config::load(&path).unwrap_or_else(|e| {
            log::error!("{e}; using defaults");
            Config::default()
        });
        Wm {
            mtm,
            rules: Rules::new(&cfg.rules),
            cfg,
            cfg_stamp: modified(&path),
            displays: Vec::new(),
            spaces: Spaces::default(),
            strips: HashMap::new(),
            windows: HashMap::new(),
            apps: HashMap::new(),
            started: false,
            enabled: true,
            own_pid: std::process::id() as Pid,
            applied: HashMap::new(),
            parked: HashSet::new(),
            min_widths: HashMap::new(),
            anims: HashMap::new(),
            dirty: HashSet::new(),
            frame_scheduled: false,
            drags: HashMap::new(),
            gesture: Gesture::default(),
            ffm_target: None,
            retry: Vec::new(),
            session_dirty: false,
        }
    }

    fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        ax::AxEl::set_global_timeout(1.0);
        self.apply_bindings();
        if let Err(e) = eventtap::install() {
            log::error!("{e}");
        }
        self.refresh_displays();

        // Collect every window first so the saved layout can be rebuilt in one go.
        let mut found: Vec<(WinId, StripKey, RuleEffect)> = Vec::new();
        for pid in self.running_regular_apps() {
            let _ = self.add_app(pid, Some(&mut found));
        }
        let live: Vec<LiveWindow> = found
            .iter()
            .map(|(id, key, _)| {
                let (app, title) = self.describe(*id);
                LiveWindow { id: *id, app, title, key: key.clone() }
            })
            .collect();
        let (strips, leftovers) = load_session().restore(&live);
        for (key, strip) in strips {
            for id in strip.windows() {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.state = WinState::Tiled(key.clone());
                }
            }
            self.strips.insert(key, strip);
        }
        for i in leftovers {
            let (id, key, effect) = found[i].clone();
            self.tile(id, key, effect, false);
        }
        self.sync_focus(false);
        self.settle_all(false);
        let n = self.windows.values().filter(|w| matches!(w.state, WinState::Tiled(_))).count();
        log::info!("managing {n} windows on {} displays", self.displays.len());
        self.housekeeping();
    }

    // ---- apps and windows ---------------------------------------------

    fn running_regular_apps(&self) -> Vec<Pid> {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .map(|a| a.processIdentifier())
            .filter(|&p| p != self.own_pid)
            .collect()
    }

    /// Start observing an app and register its windows. With `collect`, tiling
    /// candidates are returned instead of being placed.
    fn add_app(&mut self, pid: Pid, mut collect: Option<&mut Vec<(WinId, StripKey, RuleEffect)>>) -> Result<(), ()> {
        if pid == self.own_pid || self.apps.contains_key(&pid) {
            return Ok(());
        }
        let Some(running) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else { return Ok(()) };
        if running.activationPolicy() != NSApplicationActivationPolicy::Regular {
            return Ok(());
        }
        let bundle = running.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
        let name = running.localizedName().map(|s| s.to_string()).unwrap_or_default();
        let Some(el) = AxEl::application(pid) else { return Ok(()) };
        let observer = AppObserver::new(pid, ax_callback).map_err(|_| ())?;
        for n in [notif::WINDOW_CREATED, notif::FOCUSED_WINDOW_CHANGED, notif::APP_HIDDEN, notif::APP_SHOWN] {
            if observer.add(&el, n, 0) == AX_ERR_CANNOT_COMPLETE {
                return Err(());
            }
        }
        let enhanced_ui = el.boolean(attr::ENHANCED_UI) == Some(true);
        self.apps.insert(pid, App { el: el.clone(), _observer: observer, bundle, name, enhanced_ui });
        for w in el.elements(attr::WINDOWS) {
            match self.track(w.clone(), pid, false) {
                Track::Registered(id, Some((key, effect))) => match collect.as_deref_mut() {
                    Some(v) => v.push((id, key, effect)),
                    None => self.tile(id, key, effect, false),
                },
                Track::Retry => self.queue_retry(w, pid),
                _ => {}
            }
        }
        Ok(())
    }

    fn add_app_with_retry(&mut self, pid: Pid, attempt: u32) {
        if self.add_app(pid, None).is_err() && attempt < 10 {
            later(Duration::from_millis(300), move |wm| wm.add_app_with_retry(pid, attempt + 1));
        }
    }

    fn remove_app(&mut self, pid: Pid) {
        let ids: Vec<WinId> = self.windows.iter().filter(|(_, w)| w.pid == pid).map(|(id, _)| *id).collect();
        for id in ids {
            self.forget_window(id);
        }
        self.apps.remove(&pid);
    }

    /// Decide what to do with a window and start observing it.
    fn track(&mut self, el: AxEl, pid: Pid, force: bool) -> Track {
        let Some(id) = el.window_id() else { return Track::Skip };
        if self.windows.contains_key(&id) {
            return Track::Skip;
        }
        let Some(app) = self.apps.get(&pid) else { return Track::Skip };
        if el.string(attr::ROLE).as_deref() != Some(ax::ROLE_WINDOW) {
            return Track::Skip;
        }
        let subrole = el.string(attr::SUBROLE).unwrap_or_default();
        if !force && (subrole.is_empty() || subrole == "AXUnknown") {
            return Track::Retry;
        }
        let title = el.string(attr::TITLE).unwrap_or_default();
        let effect = self.rules.effect(&app.bundle, &app.name, &title);

        let mut state = if el.boolean(attr::MINIMIZED) == Some(true) {
            WinState::Minimized
        } else if el.boolean(attr::FULL_SCREEN) == Some(true) {
            WinState::Floating(Float::FullScreen)
        } else if effect.float == Some(true) {
            WinState::Floating(Float::Rule)
        } else if effect.float == Some(false) {
            WinState::Tiled(StripKey { display: String::new(), space: 0 })
        } else if subrole != ax::SUBROLE_STANDARD || !el.settable(attr::SIZE) || zoom_disabled(&el) {
            WinState::Floating(Float::Auto)
        } else {
            WinState::Tiled(StripKey { display: String::new(), space: 0 })
        };

        let observer = &app._observer;
        if observer.add(&el, notif::DESTROYED, id as usize) == AX_ERR_INVALID_ELEMENT {
            return Track::Skip;
        }
        for n in [notif::MINIATURIZED, notif::DEMINIATURIZED, notif::MOVED, notif::RESIZED] {
            observer.add(&el, n, id as usize);
        }

        let mut tile = None;
        if matches!(state, WinState::Tiled(_)) {
            let key = self.key_for(id, el.frame());
            if self.display_enabled(&key.display) {
                tile = Some((key, effect));
            }
            state = WinState::Floating(if tile.is_some() { Float::Auto } else { Float::Display });
        }
        self.windows.insert(id, Win { pid, el, state });
        Track::Registered(id, tile)
    }

    fn queue_retry(&mut self, el: AxEl, pid: Pid) {
        let first = self.retry.is_empty();
        self.retry.push((el, pid, 0));
        if first {
            later(Duration::from_millis(150), Wm::run_retries);
        }
    }

    /// Windows that had no subrole yet when they appeared.
    fn run_retries(&mut self) {
        let items = std::mem::take(&mut self.retry);
        for (el, pid, n) in items {
            match self.track(el.clone(), pid, n >= 3) {
                Track::Retry => self.retry.push((el, pid, n + 1)),
                Track::Registered(id, Some((key, effect))) => {
                    let focus = self.is_frontmost(pid);
                    self.tile(id, key, effect, focus);
                }
                _ => {}
            }
        }
        if !self.retry.is_empty() {
            later(Duration::from_millis(150), Wm::run_retries);
        }
    }

    fn tile(&mut self, id: WinId, key: StripKey, effect: RuleEffect, focus: bool) {
        let width = effect.width.unwrap_or(self.cfg.layout.default_width);
        let at = effect.open_at.unwrap_or(self.cfg.layout.open_at);
        self.strips.entry(key.clone()).or_default().insert_window(id, width, at, focus);
        if let Some(w) = self.windows.get_mut(&id) {
            w.state = WinState::Tiled(key.clone());
        }
        self.applied.remove(&id);
        self.settle(&key, true);
        self.session_dirty = true;
    }

    /// Take a tiled window out of its strip. Returns the strip it was in.
    fn untile(&mut self, id: WinId) -> Option<StripKey> {
        let key = match &self.windows.get(&id)?.state {
            WinState::Tiled(k) => k.clone(),
            _ => return None,
        };
        if let Some(s) = self.strips.get_mut(&key) {
            s.remove_window(id);
        }
        self.parked.remove(&id);
        self.applied.remove(&id);
        self.settle(&key, true);
        self.session_dirty = true;
        Some(key)
    }

    fn forget_window(&mut self, id: WinId) {
        let Some(win) = self.windows.get(&id) else { return };
        let was_focused = match &win.state {
            WinState::Tiled(k) => self.strips.get(k).and_then(Strip::focused_window) == Some(id),
            _ => false,
        };
        let key = self.untile(id);
        self.windows.remove(&id);
        self.min_widths.remove(&id);
        self.drags.remove(&id);
        if let (Some(key), true) = (key, was_focused) {
            later(Duration::from_millis(120), move |wm| wm.refocus(&key));
        }
    }

    /// After the focused window closed: if macOS did not move focus to some
    /// other known window, focus the strip's new focused column.
    fn refocus(&mut self, key: &StripKey) {
        if self.os_focused_id().is_some_and(|id| self.windows.contains_key(&id)) {
            return;
        }
        if let Some(id) = self.strips.get(key).and_then(Strip::focused_window) {
            self.focus_os(id);
        }
    }

    fn effect_for(&self, id: WinId) -> RuleEffect {
        let Some(w) = self.windows.get(&id) else { return RuleEffect::default() };
        let Some(app) = self.apps.get(&w.pid) else { return RuleEffect::default() };
        let title = w.el.string(attr::TITLE).unwrap_or_default();
        self.rules.effect(&app.bundle, &app.name, &title)
    }

    fn describe(&self, id: WinId) -> (String, String) {
        let Some(w) = self.windows.get(&id) else { return Default::default() };
        let app = self
            .apps
            .get(&w.pid)
            .map(|a| if a.bundle.is_empty() { a.name.clone() } else { a.bundle.clone() })
            .unwrap_or_default();
        (app, w.el.string(attr::TITLE).unwrap_or_default())
    }

    // ---- displays and spaces --------------------------------------------

    fn refresh_displays(&mut self) {
        self.displays = display::displays(self.mtm);
        let main = self.displays.first().map(|d| d.uuid.clone()).unwrap_or_default();
        self.spaces = display::spaces(&main);
        let separate = NSScreen::screensHaveSeparateSpaces(self.mtm);
        let main_space = self.spaces.current.get(&main).copied().unwrap_or(0);
        for d in &mut self.displays {
            d.space = if separate { self.spaces.current.get(&d.uuid).copied().unwrap_or(0) } else { main_space };
        }
    }

    fn display(&self, uuid: &str) -> Option<&Display> {
        self.displays.iter().find(|d| d.uuid == uuid)
    }

    /// Display containing a point, or the nearest one.
    fn display_near(&self, x: f64, y: f64) -> Option<&Display> {
        self.displays.iter().find(|d| d.frame.contains(x, y)).or_else(|| {
            self.displays.iter().min_by(|a, b| {
                let da = dist_to_rect(&a.frame, x, y);
                let db = dist_to_rect(&b.frame, x, y);
                da.total_cmp(&db)
            })
        })
    }

    fn display_enabled(&self, uuid: &str) -> bool {
        let name = self.display(uuid).map(|d| d.name.as_str()).unwrap_or("");
        !self
            .cfg
            .displays
            .disabled
            .iter()
            .any(|d| d.eq_ignore_ascii_case(uuid) || (!name.is_empty() && d.eq_ignore_ascii_case(name)))
    }

    fn is_visible(&self, key: &StripKey) -> bool {
        self.display(&key.display).is_some_and(|d| d.space == key.space || d.space == 0 || key.space == 0)
            && self.display_enabled(&key.display)
    }

    fn key_for(&self, id: WinId, frame: Option<Rect>) -> StripKey {
        let space = display::spaces_for_window(id).first().copied().unwrap_or(0);
        let separate = NSScreen::screensHaveSeparateSpaces(self.mtm);
        let by_space = if separate && space != 0 {
            self.spaces.owner.get(&space).filter(|u| self.display(u).is_some()).cloned()
        } else {
            None
        };
        let uuid = by_space
            .or_else(|| frame.and_then(|f| self.display_near(f.center().0, f.center().1)).map(|d| d.uuid.clone()))
            .or_else(|| self.displays.first().map(|d| d.uuid.clone()))
            .unwrap_or_default();
        let space = if space != 0 { space } else { self.display(&uuid).map_or(0, |d| d.space) };
        StripKey { display: uuid, space }
    }

    fn key_at(&self, x: f64, y: f64) -> Option<StripKey> {
        let d = self.display_near(x, y)?;
        self.display_enabled(&d.uuid).then(|| StripKey { display: d.uuid.clone(), space: d.space })
    }

    /// Enabled displays, left to right.
    fn display_order(&self) -> Vec<Display> {
        let mut v: Vec<Display> = self.displays.iter().filter(|d| self.display_enabled(&d.uuid)).cloned().collect();
        v.sort_by(|a, b| a.frame.x.total_cmp(&b.frame.x).then(a.frame.y.total_cmp(&b.frame.y)));
        v
    }

    fn viewport(&self, d: &Display) -> Rect {
        let (t, r, b, l) = self.cfg.layout.outer_gap.sides();
        d.visible.inset(t, r, b, l)
    }

    /// Edges of a display that touch another display.
    fn blocked(&self, d: &Display) -> Blocked {
        let touching = |edge: f64, other_edge: fn(&Rect) -> f64| {
            self.displays.iter().any(|o| {
                o.uuid != d.uuid && (other_edge(&o.frame) - edge).abs() < 2.0 && o.frame.overlaps_vertically(&d.frame)
            })
        };
        Blocked { left: touching(d.frame.x, Rect::max_x), right: touching(d.frame.max_x(), |r| r.x) }
    }

    fn geometry(&self, key: &StripKey) -> Option<(Rect, Geometry)> {
        let d = self.display(&key.display)?;
        let strip = self.strips.get(key)?;
        let vp = self.viewport(d);
        Some((vp, Geometry::new(strip, vp, self.cfg.layout.gap, &self.min_widths)))
    }

    /// Move windows whose Space or display changed into the right strip.
    fn reconcile(&mut self) {
        let separate = NSScreen::screensHaveSeparateSpaces(self.mtm);
        let mut moves = Vec::new();
        for (id, w) in &self.windows {
            let WinState::Tiled(key) = &w.state else { continue };
            let display_gone = self.display(&key.display).is_none();
            let space = display::spaces_for_window(*id).first().copied().unwrap_or(0);
            let new_key = if display_gone {
                self.key_for(*id, w.el.frame())
            } else if space != 0 && space != key.space {
                let display = if separate { self.spaces.owner.get(&space).cloned() } else { None };
                StripKey { display: display.unwrap_or_else(|| key.display.clone()), space }
            } else {
                continue;
            };
            if new_key != *key {
                moves.push((*id, key.clone(), new_key));
            }
        }
        for (id, old, new) in moves {
            if let Some(s) = self.strips.get_mut(&old) {
                s.remove_window(id);
            }
            self.dirty.insert(old);
            let width = self.cfg.layout.default_width;
            self.strips.entry(new.clone()).or_default().insert_window(id, width, OpenAt::End, false);
            if let Some(w) = self.windows.get_mut(&id) {
                w.state = WinState::Tiled(new.clone());
            }
            self.applied.remove(&id);
            self.dirty.insert(new);
            self.session_dirty = true;
        }
        let live: HashSet<String> = self.displays.iter().map(|d| d.uuid.clone()).collect();
        self.strips.retain(|k, s| !s.is_empty() || live.contains(&k.display));
    }

    // ---- layout and animation ------------------------------------------

    /// Scroll the strip so its focused column is placed per the centre mode.
    fn settle(&mut self, key: &StripKey, animate: bool) {
        let Some((_, geo)) = self.geometry(key) else {
            self.dirty.insert(key.clone());
            return;
        };
        let Some(strip) = self.strips.get(key) else { return };
        let target = geo.desired_scroll(strip, self.cfg.layout.center);
        let rest = self.anims.get(key).map(|(s, _)| s.to);
        if rest.is_some_and(|r| (r - target).abs() < 0.5) {
            self.dirty.insert(key.clone());
            self.request_frame();
            return;
        }
        self.scroll_to(key, target, animate, None);
    }

    fn settle_all(&mut self, animate: bool) {
        let keys: Vec<StripKey> = self.strips.keys().cloned().collect();
        for k in keys {
            self.settle(&k, animate);
        }
    }

    fn scroll_to(&mut self, key: &StripKey, target: f64, animate: bool, velocity: Option<f64>) {
        let animate = animate && self.cfg.animation.enabled && self.is_visible(key);
        let Some(strip) = self.strips.get_mut(key) else { return };
        let now = Instant::now();
        let (x, v) = match self.anims.get(key) {
            Some((s, t0)) => s.sample((now - *t0).as_secs_f64()),
            None => (strip.scroll, 0.0),
        };
        let v = velocity.unwrap_or(v);
        if !animate || ((x - target).abs() < 0.5 && v.abs() < 5.0) {
            self.anims.remove(key);
            strip.scroll = target;
        } else {
            strip.scroll = x;
            self.anims.insert(key.clone(), (Spring::new(x, target, v, self.cfg.animation.speed), now));
        }
        self.dirty.insert(key.clone());
        self.request_frame();
    }

    fn request_frame(&mut self) {
        if self.frame_scheduled {
            return;
        }
        self.frame_scheduled = true;
        let delay = if self.anims.is_empty() && self.gesture.key.is_none() { 0 } else { 8 };
        later(Duration::from_millis(delay), Wm::frame);
    }

    fn frame(&mut self) {
        self.frame_scheduled = false;
        let now = Instant::now();
        let mut done = Vec::new();
        for (key, (spring, t0)) in &self.anims {
            let t = (now - *t0).as_secs_f64();
            if let Some(s) = self.strips.get_mut(key) {
                s.scroll = if spring.done(t) { spring.to } else { spring.sample(t).0 };
            }
            if spring.done(t) {
                done.push(key.clone());
            }
            self.dirty.insert(key.clone());
        }
        for k in done {
            self.anims.remove(&k);
        }
        let keys: Vec<StripKey> = self.dirty.drain().collect();
        for k in keys {
            self.apply(&k);
        }
        if !self.anims.is_empty() {
            self.request_frame();
        }
    }

    /// Push the strip's computed frames to the windows.
    fn apply(&mut self, key: &StripKey) {
        if !self.enabled || !self.is_visible(key) {
            return;
        }
        let Some(display) = self.display(&key.display).cloned() else { return };
        let Some(strip) = self.strips.get(key) else { return };
        let viewport = self.viewport(&display);
        let blocked = self.blocked(&display);
        let params = LayoutParams { gap: self.cfg.layout.gap, center: self.cfg.layout.center };
        let (_, placements) = place(strip, viewport, &params, blocked, &self.min_widths);
        let dragging = !self.drags.is_empty() && display::left_button_down();

        let mut dead = Vec::new();
        let mut remeasure = false;
        let mut quieted: Vec<Pid> = Vec::new();
        for p in placements {
            if dragging && self.drags.contains_key(&p.id) {
                continue;
            }
            let Some(win) = self.windows.get(&p.id) else { continue };
            let t = match p.parked {
                Some(side) => park_rect(&display, blocked, p.frame, side),
                None => p.frame,
            };
            let t = Rect::new(t.x.round(), t.y.round(), t.w.round(), t.h.round());
            let old = self.applied.get(&p.id);
            if old.is_some_and(|o| o.approx_eq(&t, 0.5)) {
                continue;
            }
            let resize = old.is_none_or(|o| (o.w - t.w).abs() > 0.5 || (o.h - t.h).abs() > 0.5);
            if !quieted.contains(&win.pid) {
                if let Some(app) = self.apps.get(&win.pid).filter(|a| a.enhanced_ui) {
                    app.el.set_bool(attr::ENHANCED_UI, false);
                    quieted.push(win.pid);
                }
            }
            let err = if resize {
                // Size, move, size: the first resize may be clamped by the
                // screen edge at the old position.
                win.el.set_size(t.w, t.h);
                win.el.set_position(t.x, t.y);
                win.el.set_size(t.w, t.h)
            } else {
                win.el.set_position(t.x, t.y)
            };
            if err == AX_ERR_INVALID_ELEMENT {
                dead.push(p.id);
                continue;
            }
            self.applied.insert(p.id, t);
            if p.parked.is_some() {
                self.parked.insert(p.id);
            } else {
                self.parked.remove(&p.id);
                if resize && err == AX_OK {
                    // Some apps enforce a minimum width; lay out around it.
                    if let Some(actual) = win.el.size() {
                        if actual.width > t.w + 2.0 && self.min_widths.get(&p.id) != Some(&actual.width) {
                            self.min_widths.insert(p.id, actual.width);
                            remeasure = true;
                        }
                    }
                }
            }
        }
        for pid in quieted {
            if let Some(app) = self.apps.get(&pid) {
                app.el.set_bool(attr::ENHANCED_UI, true);
            }
        }
        for id in dead {
            self.forget_window(id);
        }
        if remeasure {
            self.dirty.insert(key.clone());
            self.request_frame();
        }
    }

    /// Bring every parked window back on screen (pausing or quitting).
    fn release_all(&mut self) {
        let parked: Vec<WinId> = self.parked.drain().collect();
        for (n, id) in parked.into_iter().enumerate() {
            let Some(w) = self.windows.get(&id) else { continue };
            let display = match &w.state {
                WinState::Tiled(k) | WinState::Hidden(k) => self.display(&k.display),
                _ => None,
            }
            .or(self.displays.first());
            let (Some(d), Some(size)) = (display, w.el.size()) else { continue };
            let v = d.visible;
            let (cw, ch) = (size.width.min(v.w), size.height.min(v.h));
            let off = 24.0 * (n % 10) as f64;
            w.el.set_position(
                v.x + ((v.w - cw) / 2.0 + off).min(v.w - cw),
                v.y + ((v.h - ch) / 2.0 + off).min(v.h - ch),
            );
        }
        self.applied.clear();
    }

    // ---- focus -----------------------------------------------------------

    fn is_frontmost(&self, pid: Pid) -> bool {
        NSWorkspace::sharedWorkspace().frontmostApplication().is_some_and(|a| a.processIdentifier() == pid)
    }

    fn os_focused(&self) -> Option<(Pid, AxEl)> {
        let pid = NSWorkspace::sharedWorkspace().frontmostApplication()?.processIdentifier();
        let el = self.apps.get(&pid)?.el.element(attr::FOCUSED_WINDOW)?;
        Some((pid, el))
    }

    fn os_focused_id(&self) -> Option<WinId> {
        self.os_focused().and_then(|(_, el)| el.window_id())
    }

    fn focus_os(&mut self, id: WinId) {
        let Some(win) = self.windows.get(&id) else { return };
        win.el.set_bool(attr::MAIN, true);
        win.el.perform(ax::ACTION_RAISE);
        if let Some(app) = self.apps.get(&win.pid) {
            app.el.set_bool(attr::FRONTMOST, true);
        }
        if !self.is_frontmost(win.pid) {
            if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(win.pid) {
                #[allow(deprecated)]
                app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
            }
        }
    }

    fn warp_to(&self, id: WinId, force: bool) {
        if !(force || self.cfg.focus.mouse_follows_focus) {
            return;
        }
        let Some(f) = self.applied.get(&id).copied().or_else(|| self.windows.get(&id).and_then(|w| w.el.frame()))
        else {
            return;
        };
        let (mx, my) = display::mouse_location();
        if !f.contains(mx, my) {
            let (cx, cy) = f.center();
            display::warp_mouse(cx, cy);
        }
    }

    /// Follow macOS focus (clicks, Cmd-Tab, apps raising windows).
    fn sync_focus(&mut self, scroll: bool) {
        let Some((pid, el)) = self.os_focused() else { return };
        let Some(id) = el.window_id() else { return };
        if !self.windows.contains_key(&id) {
            if let Track::Registered(id, Some((key, effect))) = self.track(el, pid, false) {
                self.tile(id, key, effect, true);
            }
            return;
        }
        let from_mouse = self.ffm_target.take() == Some(id);
        let WinState::Tiled(key) = self.windows[&id].state.clone() else { return };
        let Some(strip) = self.strips.get_mut(&key) else { return };
        if strip.focus_window(id) {
            self.session_dirty = true;
            if scroll && !from_mouse {
                self.settle(&key, true);
            } else {
                self.dirty.insert(key);
                self.request_frame();
            }
        } else if scroll && !from_mouse && !self.focused_visible_at_rest(&key) {
            self.settle(&key, true);
        }
    }

    fn focused_visible_at_rest(&self, key: &StripKey) -> bool {
        let (Some(strip), Some((_, geo))) = (self.strips.get(key), self.geometry(key)) else { return true };
        let scroll = self.anims.get(key).map_or(strip.scroll, |(s, _)| s.to);
        strip.focus >= strip.len() || geo.fully_visible(strip, strip.focus, scroll)
    }

    /// Strip to act on: the focused window's, else the one under the mouse.
    fn active_key(&self) -> Option<StripKey> {
        if let Some(id) = self.os_focused_id() {
            if let Some(Win { state: WinState::Tiled(k), .. }) = self.windows.get(&id) {
                return Some(k.clone());
            }
        }
        let (x, y) = display::mouse_location();
        self.key_at(x, y)
    }

    // ---- actions ---------------------------------------------------------

    fn action(&mut self, a: Action) -> Result<String, String> {
        if !self.enabled && !a.works_when_paused() {
            return Err("tiling is paused".into());
        }
        match a {
            Action::ToggleTiling => return Ok(self.set_enabled(!self.enabled)),
            Action::ReloadConfig => return self.reload_config(),
            Action::Quit => self.quit(),
            Action::Retile => {
                self.retile();
                return Ok("ok".into());
            }
            Action::ToggleFloat => return self.toggle_float(),
            Action::SendToNextDisplay => return self.send_to_display(true),
            Action::SendToPrevDisplay => return self.send_to_display(false),
            Action::FocusNextDisplay => return self.focus_display(true),
            Action::FocusPrevDisplay => return self.focus_display(false),
            _ => {}
        }

        let key = self.active_key().ok_or("no strip here")?;
        let l = &self.cfg.layout;
        let (presets, wstep, hstep) = (l.width_presets.clone(), l.width_step, l.height_step);
        let strip = self.strips.get_mut(&key).filter(|s| !s.is_empty()).ok_or("no windows on this strip")?;
        let before = strip.focused_window();
        let mut resized = false;
        use Action::*;
        let changed = match a {
            Focus(d) => strip.focus_dir(d),
            FocusFirst => strip.focus_first(),
            FocusLast => strip.focus_last(),
            FocusColumn(n) => strip.focus_index(n - 1),
            Move(d) => strip.move_column(d),
            MoveFirst => strip.move_column_to(0),
            MoveLast => strip.move_column_to(usize::MAX),
            MoveToColumn(n) => strip.move_column_to(n - 1),
            StackLeft => strip.stack_into(Side::Left),
            StackRight => strip.stack_into(Side::Right),
            PopOutLeft => strip.pop_out(Side::Left),
            PopOutRight => strip.pop_out(Side::Right),
            ToggleTabbed => strip.toggle_tabbed(),
            CycleWidth => {
                resized = true;
                strip.cycle_width(&presets, true)
            }
            CycleWidthBack => {
                resized = true;
                strip.cycle_width(&presets, false)
            }
            SetWidth(w) => {
                resized = true;
                strip.set_width(w)
            }
            Wider => {
                resized = true;
                strip.adjust_width(wstep)
            }
            Narrower => {
                resized = true;
                strip.adjust_width(-wstep)
            }
            FullWidth => {
                resized = true;
                strip.toggle_full_width()
            }
            Taller => strip.adjust_height(hstep),
            Shorter => strip.adjust_height(-hstep),
            EqualizeHeights => strip.equalize_heights(),
            PinLeft => strip.toggle_pin(Side::Left),
            PinRight => strip.toggle_pin(Side::Right),
            Center => true,
            _ => false,
        };
        let after = strip.focused_window();
        if resized {
            if let Some(col) = strip.focused_column() {
                for w in &col.windows {
                    self.min_widths.remove(w);
                }
            }
        }
        if changed {
            if a == Center {
                self.center_focused(&key);
            } else {
                self.settle(&key, true);
            }
            self.session_dirty = true;
        }
        if let Some(id) = after {
            if after != before || self.os_focused_id() != Some(id) {
                self.focus_os(id);
                self.warp_to(id, false);
            }
        }
        Ok(if changed { "ok" } else { "nothing to do" }.into())
    }

    fn center_focused(&mut self, key: &StripKey) {
        let Some((vp, geo)) = self.geometry(key) else { return };
        let Some(strip) = self.strips.get(key) else { return };
        let f = strip.focus;
        if strip.columns.get(f).is_none_or(|c| c.pin.is_some()) {
            return;
        }
        let _ = vp;
        let target = geo.offsets[f] + geo.widths[f] / 2.0 - geo.area.w / 2.0;
        let target = geo.clamp_scroll(target, self.cfg.layout.center);
        self.scroll_to(key, target, true, None);
    }

    fn toggle_float(&mut self) -> Result<String, String> {
        let (pid, el) = self.os_focused().ok_or("no focused window")?;
        let id = el.window_id().ok_or("focused element is not a window")?;
        if !self.windows.contains_key(&id) {
            self.track(el.clone(), pid, true);
        }
        let state = self.windows.get(&id).map(|w| w.state.clone()).ok_or("window is not managed")?;
        match state {
            WinState::Tiled(key) => {
                self.untile(id);
                if let Some(w) = self.windows.get_mut(&id) {
                    w.state = WinState::Floating(Float::User);
                }
                if let (Some(d), Some(size)) = (self.display(&key.display), el.size()) {
                    let v = d.visible;
                    let (w, h) = (size.width.min(v.w * 0.8), size.height.min(v.h * 0.8));
                    el.set_size(w, h);
                    el.set_position(v.x + (v.w - w) / 2.0, v.y + (v.h - h) / 2.0);
                }
                el.perform(ax::ACTION_RAISE);
                Ok("floating".into())
            }
            _ => {
                let key = self.key_for(id, el.frame());
                if !self.display_enabled(&key.display) {
                    return Err("this display is not managed".into());
                }
                let effect = RuleEffect { float: None, ..self.effect_for(id) };
                self.tile(id, key, effect, true);
                Ok("tiled".into())
            }
        }
    }

    fn send_to_display(&mut self, next: bool) -> Result<String, String> {
        let id = self.os_focused_id().ok_or("no focused window")?;
        let Some(WinState::Tiled(key)) = self.windows.get(&id).map(|w| w.state.clone()) else {
            return Err("focused window is not tiled".into());
        };
        let order = self.display_order();
        if order.len() < 2 {
            return Err("only one managed display".into());
        }
        let i = order.iter().position(|d| d.uuid == key.display).unwrap_or(0);
        let j = if next { (i + 1) % order.len() } else { (i + order.len() - 1) % order.len() };
        let target = StripKey { display: order[j].uuid.clone(), space: order[j].space };
        let width = self
            .strips
            .get(&key)
            .and_then(|s| s.find(id).map(|(c, _)| s.columns[c].width))
            .unwrap_or(self.cfg.layout.default_width);
        self.untile(id);
        self.min_widths.remove(&id);
        self.strips.entry(target.clone()).or_default().insert_window(id, width, OpenAt::Next, true);
        if let Some(w) = self.windows.get_mut(&id) {
            w.state = WinState::Tiled(target.clone());
        }
        self.settle(&target, false);
        self.frame();
        self.focus_os(id);
        self.warp_to(id, false);
        Ok("ok".into())
    }

    fn focus_display(&mut self, next: bool) -> Result<String, String> {
        let order = self.display_order();
        if order.len() < 2 {
            return Err("only one managed display".into());
        }
        let cur = self.active_key().map(|k| k.display).unwrap_or_default();
        let i = order.iter().position(|d| d.uuid == cur).unwrap_or(0);
        let j = if next { (i + 1) % order.len() } else { (i + order.len() - 1) % order.len() };
        let d = &order[j];
        let key = StripKey { display: d.uuid.clone(), space: d.space };
        match self.strips.get(&key).and_then(Strip::focused_window) {
            Some(id) => {
                self.focus_os(id);
                self.warp_to(id, false);
            }
            None => {
                let (cx, cy) = d.visible.center();
                display::warp_mouse(cx, cy);
            }
        }
        Ok("ok".into())
    }

    fn set_enabled(&mut self, on: bool) -> String {
        if on == self.enabled {
            return if on { "tiling is on" } else { "tiling is paused" }.into();
        }
        self.enabled = on;
        eventtap::set_paused(!on);
        menu::set_paused(!on);
        if on {
            self.retile();
            "tiling is on".into()
        } else {
            self.anims.clear();
            self.release_all();
            "tiling is paused".into()
        }
    }

    /// Re-read everything from the system and lay out again.
    fn retile(&mut self) {
        self.refresh_displays();
        let dead: Vec<WinId> = self.windows.iter().filter(|(_, w)| !alive(&w.el)).map(|(id, _)| *id).collect();
        for id in dead {
            self.forget_window(id);
        }
        let running: HashSet<Pid> = self.running_regular_apps().into_iter().collect();
        let gone: Vec<Pid> = self.apps.keys().filter(|p| !running.contains(p)).copied().collect();
        for pid in gone {
            self.remove_app(pid);
        }
        for pid in running {
            if self.apps.contains_key(&pid) {
                self.scan_app(pid);
            } else {
                let _ = self.add_app(pid, None);
            }
        }
        self.reconcile();
        self.applied.clear();
        self.settle_all(false);
        self.sync_focus(true);
    }

    /// Pick up windows that exist but were not seen yet (e.g. on another Space).
    fn scan_app(&mut self, pid: Pid) {
        let Some(app) = self.apps.get(&pid) else { return };
        for w in app.el.elements(attr::WINDOWS) {
            match self.track(w.clone(), pid, false) {
                Track::Registered(id, Some((key, effect))) => self.tile(id, key, effect, false),
                Track::Retry => self.queue_retry(w, pid),
                _ => {}
            }
        }
    }

    fn reload_config(&mut self) -> Result<String, String> {
        let path = config::config_path();
        self.cfg_stamp = modified(&path);
        let cfg = Config::load(&path).inspect_err(|e| log::error!("config not reloaded: {e}"))?;
        self.cfg = cfg;
        self.rules = Rules::new(&self.cfg.rules);
        let warnings = self.apply_bindings();
        if self.enabled {
            self.refresh_displays();
            self.applied.clear();
            self.settle_all(false);
        }
        log::info!("config reloaded");
        Ok(if warnings.is_empty() {
            "config reloaded".into()
        } else {
            format!("config reloaded; skipped bindings:\n  {}", warnings.join("\n  "))
        })
    }

    /// Resolve bindings against the current keyboard layout and hand them to
    /// the key tap.
    fn apply_bindings(&mut self) -> Vec<String> {
        let layout = keymap::current_layout();
        let (bindings, warnings) = self.cfg.bindings();
        let lookup = |c: char| layout.get(&c).copied();
        let map: HashMap<(u16, Mods), Action> =
            bindings.iter().filter_map(|(k, a)| k.resolve(&lookup).map(|r| (r, *a))).collect();
        eventtap::set_bindings(map);
        eventtap::set_track_mouse_moves(self.cfg.focus.follows_mouse);
        for w in &warnings {
            log::warn!("{w}");
        }
        warnings
    }

    fn quit(&mut self) -> ! {
        log::info!("quitting");
        self.release_all();
        self.save_session();
        ipc::remove_socket();
        std::process::exit(0)
    }

    // ---- events ----------------------------------------------------------

    fn on_ax(&mut self, name: &str, el: AxEl, refcon: usize) {
        if !self.enabled || !self.started {
            return;
        }
        let id = refcon as WinId;
        match name {
            notif::WINDOW_CREATED => {
                let Some(pid) = el.pid() else { return };
                match self.track(el.clone(), pid, false) {
                    Track::Registered(id, Some((key, effect))) => {
                        let focus = self.is_frontmost(pid);
                        self.tile(id, key, effect, focus);
                    }
                    Track::Retry => self.queue_retry(el, pid),
                    _ => {}
                }
            }
            notif::FOCUSED_WINDOW_CHANGED => self.sync_focus(true),
            notif::DESTROYED => self.forget_window(id),
            notif::MINIATURIZED => {
                if self.untile(id).is_some() {
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.state = WinState::Minimized;
                    }
                }
            }
            notif::DEMINIATURIZED => {
                if self.windows.get(&id).is_some_and(|w| w.state == WinState::Minimized) {
                    self.retile_window(id, true);
                }
            }
            notif::MOVED => self.on_moved(id, false),
            notif::RESIZED => self.on_moved(id, true),
            notif::APP_HIDDEN => {
                let Some(pid) = el.pid() else { return };
                let ids: Vec<WinId> = self
                    .windows
                    .iter()
                    .filter(|(_, w)| w.pid == pid && matches!(w.state, WinState::Tiled(_)))
                    .map(|(id, _)| *id)
                    .collect();
                for id in ids {
                    if let Some(key) = self.untile(id) {
                        if let Some(w) = self.windows.get_mut(&id) {
                            w.state = WinState::Hidden(key);
                        }
                    }
                }
            }
            notif::APP_SHOWN => {
                let Some(pid) = el.pid() else { return };
                let ids: Vec<(WinId, StripKey)> = self
                    .windows
                    .iter()
                    .filter_map(|(id, w)| match &w.state {
                        WinState::Hidden(k) if w.pid == pid => Some((*id, k.clone())),
                        _ => None,
                    })
                    .collect();
                for (id, key) in ids {
                    let effect = self.effect_for(id);
                    self.tile(id, key, effect, false);
                }
            }
            _ => {}
        }
    }

    /// Put a floating/minimized window back on the strip of its display.
    fn retile_window(&mut self, id: WinId, focus: bool) {
        let Some(frame) = self.windows.get(&id).map(|w| w.el.frame()) else { return };
        let key = self.key_for(id, frame);
        if self.display_enabled(&key.display) {
            let effect = self.effect_for(id);
            self.tile(id, key, effect, focus);
        } else if let Some(w) = self.windows.get_mut(&id) {
            w.state = WinState::Floating(Float::Display);
        }
    }

    fn on_moved(&mut self, id: WinId, resized: bool) {
        let Some(win) = self.windows.get(&id) else { return };
        match win.state.clone() {
            WinState::Tiled(_) => {
                if resized && win.el.boolean(attr::FULL_SCREEN) == Some(true) {
                    self.untile(id);
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.state = WinState::Floating(Float::FullScreen);
                    }
                    return;
                }
                // Notifications also arrive for frames Stripline set itself;
                // only a frame that differs from ours is the user's doing.
                if self.parked.contains(&id) || self.drags.contains_key(&id) {
                    return;
                }
                let (Some(ours), Some(actual)) = (self.applied.get(&id).copied(), win.el.frame()) else { return };
                if actual.approx_eq(&ours, 4.0) {
                    return;
                }
                let kind = if (actual.w - ours.w).abs() > 4.0 || (actual.h - ours.h).abs() > 4.0 {
                    Drag::Resize
                } else {
                    Drag::Move
                };
                self.drags.insert(id, kind);
                if !display::left_button_down() {
                    // The drag already ended before this notification arrived.
                    let (mx, my) = display::mouse_location();
                    let near = actual.inset(-12.0, -12.0, -12.0, -12.0);
                    if near.contains(mx, my) {
                        self.on_mouse_up(mx, my);
                    } else {
                        // Moved by the app itself: put it back.
                        self.drags.remove(&id);
                        self.applied.remove(&id);
                        if let WinState::Tiled(key) = win.state.clone() {
                            self.dirty.insert(key);
                            self.request_frame();
                        }
                    }
                }
            }
            WinState::Floating(Float::FullScreen) if resized && win.el.boolean(attr::FULL_SCREEN) == Some(false) => {
                self.retile_window(id, true);
            }
            _ => {}
        }
    }

    fn on_mouse_up(&mut self, x: f64, y: f64) {
        if self.drags.is_empty() || !self.enabled {
            return;
        }
        let drags: Vec<(WinId, Drag)> = self.drags.drain().collect();
        for (id, kind) in drags {
            let Some(WinState::Tiled(key)) = self.windows.get(&id).map(|w| w.state.clone()) else { continue };
            self.applied.remove(&id);
            match kind {
                Drag::Resize => self.finish_resize(id, &key),
                Drag::Move => self.finish_move(id, &key, x, y),
            }
            self.session_dirty = true;
        }
    }

    /// The user resized a tiled window with the mouse: adopt its new width.
    fn finish_resize(&mut self, id: WinId, key: &StripKey) {
        let size = self.windows.get(&id).and_then(|w| w.el.size());
        let geo = self.geometry(key);
        let gap = self.cfg.layout.gap;
        if let (Some(size), Some((vp, geo)), Some(strip)) = (size, geo, self.strips.get_mut(key)) {
            if let Some((c, _)) = strip.find(id) {
                let avail = if strip.columns[c].pin.is_some() { vp.w } else { geo.area.w };
                let col = &mut strip.columns[c];
                col.width = ((size.width + gap) / (avail + gap)).clamp(MIN_WIDTH, MAX_WIDTH);
                col.restore_width = None;
                for w in col.windows.clone() {
                    self.min_widths.remove(&w);
                    self.applied.remove(&w);
                }
            }
        }
        self.settle(key, true);
    }

    /// The user dragged a tiled window: reorder, stack, or move to another display.
    fn finish_move(&mut self, id: WinId, key: &StripKey, x: f64, y: f64) {
        let gap = self.cfg.layout.gap;
        let target = self.key_at(x, y);
        match target {
            Some(t) if t == *key => {
                if let (Some((vp, geo)), Some(strip)) = (self.geometry(key), self.strips.get(key)) {
                    let to = geo.column_at(strip, vp, gap, x);
                    let strip = self.strips.get_mut(key).unwrap();
                    if let (Some((c, _)), Some(to)) = (strip.find(id), to) {
                        if to != c {
                            if strip.columns[c].len() == 1 {
                                strip.reorder_column(c, to);
                            } else {
                                strip.move_window_to_column(id, to);
                            }
                        }
                    }
                }
                self.settle(key, true);
            }
            Some(t) => {
                let width = self
                    .strips
                    .get(key)
                    .and_then(|s| s.find(id).map(|(c, _)| s.columns[c].width))
                    .unwrap_or(self.cfg.layout.default_width);
                self.untile(id);
                self.strips.entry(t.clone()).or_default();
                let idx = match (self.geometry(&t), self.strips.get(&t)) {
                    (Some((vp, geo)), Some(s)) => geo.insert_index_at(s, vp, gap, x),
                    _ => 0,
                };
                self.strips.get_mut(&t).unwrap().insert_window_at(id, width, idx, true);
                if let Some(w) = self.windows.get_mut(&id) {
                    w.state = WinState::Tiled(t.clone());
                }
                self.settle(&t, true);
            }
            None => self.settle(key, true),
        }
    }

    /// Focus follows mouse: focus the tiled window under the pointer, without
    /// scrolling the strip.
    fn on_mouse_moved(&mut self, x: f64, y: f64) {
        if !self.enabled || !self.cfg.focus.follows_mouse || display::left_button_down() || self.gesture.key.is_some() {
            return;
        }
        let Some(el) = AxEl::window_at(x, y) else { return };
        let Some(id) = el.window_id() else { return };
        if !self.windows.get(&id).is_some_and(|w| matches!(w.state, WinState::Tiled(_))) {
            return;
        }
        if self.os_focused_id() == Some(id) {
            return;
        }
        self.ffm_target = Some(id);
        self.focus_os(id);
    }

    /// Trackpad swipe: the strip follows the fingers, then settles on a column.
    fn on_gesture(&mut self, n: usize, x: f64, y: f64) {
        if !self.enabled || !self.cfg.gestures.enabled {
            return;
        }
        let fingers = self.cfg.gestures.fingers as usize;
        let now = Instant::now();
        if n != fingers {
            let g = std::mem::take(&mut self.gesture);
            if let (Some(key), true) = (g.key, g.horizontal) {
                self.finish_swipe(&key, g.velocity);
            }
            return;
        }
        if self.gesture.ignore {
            return;
        }
        let Some(key) = self.gesture.key.clone() else {
            let (mx, my) = display::mouse_location();
            match self.key_at(mx, my).filter(|k| self.strips.get(k).is_some_and(|s| !s.is_empty())) {
                Some(key) => {
                    // Catch the strip where it is if it was still moving.
                    if let Some((s, t0)) = self.anims.remove(&key) {
                        if let Some(st) = self.strips.get_mut(&key) {
                            st.scroll = s.sample((now - t0).as_secs_f64()).0;
                        }
                    }
                    self.gesture =
                        Gesture { key: Some(key), start: (x, y), last_x: x, last_t: Some(now), ..Default::default() };
                }
                None => self.gesture.ignore = true,
            }
            return;
        };
        let g = &mut self.gesture;
        if !g.horizontal {
            let (dx, dy) = (x - g.start.0, y - g.start.1);
            if dx.abs().max(dy.abs()) < 0.015 {
                return;
            }
            if dy.abs() > dx.abs() {
                // Vertical swipe: leave it to macOS (Mission Control etc).
                *g = Gesture { ignore: true, ..Default::default() };
                return;
            }
            g.horizontal = true;
        }
        let dx = x - g.last_x;
        let dt = g.last_t.map_or(0.016, |t| (now - t).as_secs_f64()).max(0.001);
        g.last_x = x;
        g.last_t = Some(now);
        let Some((_, geo)) = self.geometry(&key) else { return };
        let dir = if self.cfg.gestures.natural { -1.0 } else { 1.0 };
        let delta = dir * dx * geo.area.w * self.cfg.gestures.sensitivity * SWIPE_GAIN;
        let max = (geo.total - geo.area.w).max(0.0);
        let slack = geo.area.w * 0.2;
        if let Some(s) = self.strips.get_mut(&key) {
            s.scroll = (s.scroll + delta).clamp(-slack, max + slack);
        }
        let g = &mut self.gesture;
        g.velocity = 0.6 * g.velocity + 0.4 * (delta / dt);
        self.dirty.insert(key);
        self.request_frame();
    }

    fn finish_swipe(&mut self, key: &StripKey, velocity: f64) {
        let Some((_, geo)) = self.geometry(key) else { return };
        let Some(strip) = self.strips.get_mut(key) else { return };
        let projected = strip.scroll + velocity * SWIPE_PROJECTION;
        let (focus, target) = geo.snap(strip, projected, self.cfg.layout.center);
        let changed = strip.focus != focus;
        strip.focus = focus;
        let focused = strip.focused_window();
        self.scroll_to(key, target, true, Some(velocity));
        if changed {
            if let Some(id) = focused {
                self.focus_os(id);
            }
            self.session_dirty = true;
        }
    }

    fn on_system_event(&mut self, ev: SystemEvent) {
        if !self.started {
            return;
        }
        match ev {
            SystemEvent::KeyboardLayoutChanged => {
                self.apply_bindings();
            }
            _ if !self.enabled => {}
            SystemEvent::AppLaunched(pid) => self.add_app_with_retry(pid, 0),
            SystemEvent::AppTerminated(pid) => self.remove_app(pid),
            SystemEvent::AppActivated(pid) => {
                if !self.apps.contains_key(&pid) {
                    let _ = self.add_app(pid, None);
                }
                self.sync_focus(true);
            }
            SystemEvent::SpaceChanged => {
                self.refresh_displays();
                let pids: Vec<Pid> = self.apps.keys().copied().collect();
                for pid in pids {
                    self.scan_app(pid);
                }
                self.reconcile();
                self.settle_all(false);
                self.sync_focus(true);
            }
            SystemEvent::ScreensChanged => {
                self.refresh_displays();
                self.reconcile();
                self.applied.clear();
                self.settle_all(false);
            }
            SystemEvent::Woke => later(Duration::from_millis(1500), Wm::retile),
        }
    }

    /// Periodic: pick up config edits and save the session.
    fn housekeeping(&mut self) {
        let stamp = modified(&config::config_path());
        if stamp != self.cfg_stamp {
            let _ = self.reload_config();
        }
        if self.session_dirty && self.gesture.key.is_none() {
            self.save_session();
        }
        later(HOUSEKEEPING, Wm::housekeeping);
    }

    fn save_session(&mut self) {
        let session = Session::capture(&self.strips, |id| self.describe(id));
        let dir = config::data_dir();
        let result = std::fs::create_dir_all(&dir).and_then(|()| {
            std::fs::write(dir.join("session.json"), serde_json::to_vec_pretty(&session).unwrap_or_default())
        });
        if let Err(e) = result {
            log::warn!("could not save session: {e}");
        }
        self.session_dirty = false;
    }

    // ---- control socket ----------------------------------------------

    fn command(&mut self, line: &str) -> String {
        let reply = match line.trim() {
            "" | "ping" => Ok("pong".into()),
            "state" => Ok(self.state_json()),
            "displays" => Ok(self.displays_json()),
            "pause" => Ok(self.set_enabled(false)),
            "resume" => Ok(self.set_enabled(true)),
            "reload" => self.reload_config(),
            "quit" => {
                // Reply first; the process exits on the next turn of the run loop.
                dispatch::Queue::main().exec_async(|| run_action(Action::Quit));
                Ok("quitting".into())
            }
            other => other.parse::<Action>().and_then(|a| self.action(a)),
        };
        reply.unwrap_or_else(|e| format!("error: {e}"))
    }

    fn displays_json(&self) -> String {
        let v: Vec<_> = self
            .displays
            .iter()
            .map(|d| {
                json!({
                    "name": d.name, "uuid": d.uuid, "space": d.space,
                    "frame": [d.frame.x, d.frame.y, d.frame.w, d.frame.h],
                    "managed": self.display_enabled(&d.uuid),
                })
            })
            .collect();
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    fn state_json(&self) -> String {
        let mut keys: Vec<&StripKey> = self.strips.keys().collect();
        keys.sort();
        let strips: Vec<_> = keys
            .into_iter()
            .map(|k| {
                let s = &self.strips[k];
                json!({
                    "display": self.display(&k.display).map_or(k.display.clone(), |d| d.name.clone()),
                    "space": k.space,
                    "visible": self.is_visible(k),
                    "scroll": s.scroll.round(),
                    "focus": s.focus,
                    "columns": s.columns.iter().map(|c| json!({
                        "width": (c.width * 1000.0).round() / 1000.0,
                        "tabbed": c.tabbed,
                        "pin": c.pin,
                        "windows": c.windows.iter().map(|&id| {
                            let (app, title) = self.describe(id);
                            json!({ "id": id, "app": app, "title": title })
                        }).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let floating: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| matches!(w.state, WinState::Floating(_)))
            .map(|(&id, w)| {
                let (app, title) = self.describe(id);
                json!({ "id": id, "app": app, "title": title, "reason": format!("{:?}", w.state) })
            })
            .collect();
        serde_json::to_string_pretty(&json!({
            "enabled": self.enabled,
            "strips": strips,
            "floating": floating,
        }))
        .unwrap_or_default()
    }
}

// ---- helpers ----------------------------------------------------------------

fn zoom_disabled(el: &AxEl) -> bool {
    el.element(attr::ZOOM_BUTTON).and_then(|z| z.boolean(attr::ENABLED)) == Some(false)
}

fn alive(el: &AxEl) -> bool {
    el.window_id().is_some() && el.string(attr::ROLE).is_some()
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn load_session() -> Session {
    let path = config::data_dir().join("session.json");
    std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Session>(&b).ok())
        .filter(|s| s.version == Session::VERSION)
        .unwrap_or_default()
}

fn dist_to_rect(r: &Rect, x: f64, y: f64) -> f64 {
    let dx = (r.x - x).max(0.0).max(x - r.max_x());
    let dy = (r.y - y).max(0.0).max(y - r.max_y());
    dx.hypot(dy)
}

/// Off-screen spot for a window that is out of view: the bottom corner of the
/// display on the given side, with a sliver left visible. If another display
/// touches that side, the opposite corner is used.
fn park_rect(d: &Display, blocked: Blocked, frame: Rect, side: Side) -> Rect {
    let side = match side {
        Side::Right if blocked.right && !blocked.left => Side::Left,
        Side::Left if blocked.left && !blocked.right => Side::Right,
        s => s,
    };
    let f = d.frame;
    let x = match side {
        Side::Right => f.max_x() - PARK_SLIVER,
        Side::Left => f.x - frame.w + PARK_SLIVER,
    };
    Rect::new(x, f.max_y() - PARK_SLIVER, frame.w, frame.h)
}
