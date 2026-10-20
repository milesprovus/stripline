//! Launch at login through a per-user LaunchAgent. launchd also restarts the
//! daemon if it crashes (but not after a clean quit).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::home;

pub const LABEL: &str = "io.github.stripline";

pub fn plist_path() -> PathBuf {
    home().join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
}

pub fn log_path() -> PathBuf {
    home().join("Library/Logs/stripline.log")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn plist(exe: &Path, log: &Path) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let log = xml_escape(&log.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>run</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>5</integer>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#
    )
}

fn uid() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "501".into())
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("launchctl").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn install() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let path = plist_path();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    if let Some(dir) = log_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, plist(&exe, &log_path())).map_err(|e| e.to_string())?;
    let domain = format!("gui/{}", uid());
    // Replace a previously loaded copy, ignoring "not loaded" errors.
    let _ = launchctl(&["bootout", &format!("{domain}/{LABEL}")]);
    launchctl(&["bootstrap", &domain, &path.to_string_lossy()])?;
    Ok(format!(
        "Installed {}\nStripline now starts at login and restarts if it crashes.\nLog: {}",
        path.display(),
        log_path().display()
    ))
}

/// Whether Stripline is set to start at login.
pub fn installed() -> bool {
    plist_path().exists()
}

/// Turn starting at login on or off without touching the running instance
/// (unlike `install`/`uninstall`, which also start or stop it right away).
pub fn set_login_item(on: bool) -> Result<String, String> {
    let path = plist_path();
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.canonicalize().unwrap_or(exe);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        if let Some(dir) = log_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&path, plist(&exe, &log_path())).map_err(|e| e.to_string())?;
        Ok("Stripline will open at login".into())
    } else {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok("Stripline won't open at login".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("Stripline won't open at login".into()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub fn uninstall() -> Result<String, String> {
    let path = plist_path();
    let _ = launchctl(&["bootout", &format!("gui/{}/{LABEL}", uid())]);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(format!("Removed {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("Service was not installed".into()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contains_program_and_escapes() {
        let p = plist(Path::new("/Apps/A&B/stripline"), Path::new("/tmp/log"));
        assert!(p.contains("<string>/Apps/A&amp;B/stripline</string>"));
        assert!(p.contains("<string>run</string>"));
        assert!(p.contains(LABEL));
    }
}
