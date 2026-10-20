//! `stripline doctor`: checks for macOS settings and apps that interfere.

use std::process::Command;

use objc2_app_kit::{NSScreen, NSWorkspace};
use objc2_foundation::MainThreadMarker;

use super::ax;
use crate::config::{self, Config};
use crate::ipc;

/// Window managers and snapping tools that fight over window frames.
const CONFLICTS: &[(&str, &str)] = &[
    ("com.knollsoft.Rectangle", "Rectangle"),
    ("com.knollsoft.Hookshot", "Hookshot"),
    ("com.crowdcafe.windowmagnet", "Magnet"),
    ("com.hegenberg.BetterSnapTool", "BetterSnapTool"),
    ("com.manytricks.Moom", "Moom"),
    ("com.amethyst.Amethyst", "Amethyst"),
    ("bobko.aerospace", "AeroSpace"),
    ("com.MrKai77.Loop", "Loop"),
    ("com.lwouis.alt-tab-macos", ""),
];

fn defaults_read(domain: &str, key: &str) -> Option<String> {
    let out = Command::new("defaults").args(["read", domain, key]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn line(ok: bool, text: &str) -> String {
    format!("{} {text}", if ok { "[ok]  " } else { "[warn]" })
}

pub fn report() -> String {
    let mut out = Vec::new();

    out.push(line(
        ax::is_trusted(false),
        "Accessibility permission for this process (when run from a terminal, this is the terminal's permission)",
    ));

    match ipc::send("ping") {
        Ok(_) => out.push(line(true, "stripline is running")),
        Err(_) => {
            out.push(line(false, "stripline is not running (start it with `stripline` or `stripline service install`)"))
        }
    }

    let cfg = match Config::load(&config::config_path()) {
        Ok(c) => {
            out.push(line(true, &format!("config {} is valid", config::config_path().display())));
            c
        }
        Err(e) => {
            out.push(line(false, &format!("config error: {e}")));
            Config::default()
        }
    };

    let running: Vec<String> = NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter_map(|a| a.bundleIdentifier().map(|b| b.to_string()))
        .collect();
    let conflicts: Vec<&str> = CONFLICTS
        .iter()
        .filter(|(id, name)| !name.is_empty() && running.iter().any(|r| r == id))
        .map(|(_, name)| *name)
        .collect();
    out.push(if conflicts.is_empty() {
        line(true, "no other window manager or snapping tool is running")
    } else {
        line(false, &format!("also running: {} (quit it, or disable its shortcuts and snapping)", conflicts.join(", ")))
    });

    if let Some(mtm) = MainThreadMarker::new() {
        out.push(line(
            NSScreen::screensHaveSeparateSpaces(mtm),
            "\"Displays have separate Spaces\" is on (System Settings > Desktop & Dock)",
        ));
    }

    let stage = defaults_read("com.apple.WindowManager", "GloballyEnabled").as_deref() == Some("1");
    out.push(line(!stage, if stage { "Stage Manager is on; turn it off" } else { "Stage Manager is off" }));

    if cfg.gestures.enabled {
        let f = cfg.gestures.fingers;
        let (swipe_key, drag_conflict) = if f == 3 {
            (
                "TrackpadThreeFingerHorizSwipeGesture",
                defaults_read("com.apple.AppleMultitouchTrackpad", "TrackpadThreeFingerDrag").as_deref() == Some("1"),
            )
        } else {
            ("TrackpadFourFingerHorizSwipeGesture", false)
        };
        let spaces_swipe = ["com.apple.AppleMultitouchTrackpad", "com.apple.driver.AppleBluetoothMultitouch.trackpad"]
            .iter()
            .any(|d| defaults_read(d, swipe_key).as_deref() == Some("2"));
        out.push(line(
            !spaces_swipe,
            &if spaces_swipe {
                format!(
                    "macOS also uses a {f}-finger horizontal swipe to switch Spaces. In System Settings > Trackpad > \
                     More Gestures, set \"Swipe between full-screen applications\" to {} fingers, or set gestures.fingers = {}",
                    if f == 3 { 4 } else { 3 },
                    if f == 3 { 4 } else { 3 }
                )
            } else {
                format!("no macOS {f}-finger horizontal swipe conflicts with the strip swipe")
            },
        ));
        if drag_conflict {
            out.push(line(false, "three-finger drag is on and swallows three-finger swipes; use gestures.fingers = 4"));
        }
    }

    out.join("\n")
}
