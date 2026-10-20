//! Stripline: a scrolling tiling window manager for macOS.

// Off macOS only the engine and its tests are built.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]

mod action;
mod config;
mod engine;
mod ipc;
mod keys;
mod service;
mod settings;

/// Tests that change environment variables hold this lock.
#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "macos")]
mod macos;

use std::process::ExitCode;

use action::Action;
use config::Config;

const USAGE: &str = "\
Stripline - a scrolling tiling window manager for macOS

USAGE:
    stripline [run]               start the window manager
    stripline msg <command>       send a command to the running instance
    stripline actions             list actions for key bindings and `msg`
    stripline bindings            show the active key bindings
    stripline init-config         write a commented config file
    stripline check-config        validate the config file
    stripline settings            open the settings window
    stripline doctor              check macOS settings that affect Stripline
    stripline service install    start at login (LaunchAgent)
    stripline service uninstall  stop starting at login

`msg` also understands: state, displays, pause, resume, reload.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let result: Result<String, String> = match arg(0) {
        None | Some("run") => run(),
        Some("msg") if args.len() > 1 => {
            ipc::send(&args[1..].join(" ")).and_then(|r| match r.strip_prefix("error: ") {
                Some(e) => Err(e.to_string()),
                None => Ok(r),
            })
        }
        Some("msg") => Err("usage: stripline msg <command>".into()),
        Some("actions") => Ok(Action::names().join("\n")),
        Some("bindings") => show_bindings(),
        Some("init-config") => init_config(),
        Some("check-config") => Config::load(&config::config_path()).map(|c| {
            let (_, warnings) = c.bindings();
            if warnings.is_empty() {
                format!("{} is valid", config::config_path().display())
            } else {
                format!("valid, but some bindings were skipped:\n  {}", warnings.join("\n  "))
            }
        }),
        Some("doctor") => doctor(),
        Some("settings") => {
            open_settings(args.iter().any(|a| a == "--no-open"), args.iter().any(|a| a == "--standalone"))
        }
        Some("service") => match arg(1) {
            Some("install") => service::install(),
            Some("uninstall") => service::uninstall(),
            _ => Err("usage: stripline service install|uninstall".into()),
        },
        Some("-h" | "--help" | "help") => Ok(USAGE.to_string()),
        Some("-V" | "--version" | "version") => Ok(format!("stripline {}", env!("CARGO_PKG_VERSION"))),
        Some(other) => Err(format!("unknown command '{other}'\n\n{USAGE}")),
    };
    match result {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("stripline: {e}");
            ExitCode::FAILURE
        }
    }
}

fn show_bindings() -> Result<String, String> {
    let cfg = Config::load(&config::config_path())?;
    let (bindings, warnings) = cfg.bindings();
    let mut lines: Vec<String> = bindings.iter().map(|(k, a)| format!("{:<32} {a}", k.to_string())).collect();
    lines.sort();
    lines.extend(warnings.iter().map(|w| format!("skipped: {w}")));
    Ok(lines.join("\n"))
}

/// Open the settings window in the running instance, or serve the settings
/// page from this process and open it in a browser.
fn open_settings(no_open: bool, standalone: bool) -> Result<String, String> {
    if !standalone && ipc::send("ping").is_ok() {
        return ipc::send("settings").map(|_| String::new());
    }
    let url = settings::start()?;
    println!("Settings: {url}");
    if !no_open {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(&url).spawn();
    }
    println!("Serving settings until you press Ctrl-C.");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    loop {
        std::thread::park();
    }
}

fn init_config() -> Result<String, String> {
    let path = config::config_path();
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, config::EXAMPLE).map_err(|e| e.to_string())?;
    Ok(format!("wrote {}", path.display()))
}

#[cfg(target_os = "macos")]
fn run() -> Result<String, String> {
    // A second copy (e.g. launchd starting the login item while one already
    // runs) exits cleanly, so launchd does not keep restarting it.
    if ipc::send("ping").is_ok() {
        return Ok("stripline is already running".into());
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    macos::run().map(|()| String::new())
}

#[cfg(not(target_os = "macos"))]
fn run() -> Result<String, String> {
    Err("the window manager only runs on macOS (the layout engine and tests build anywhere)".into())
}

#[cfg(target_os = "macos")]
fn doctor() -> Result<String, String> {
    Ok(macos::doctor::report(true))
}

#[cfg(not(target_os = "macos"))]
fn doctor() -> Result<String, String> {
    Err("doctor only runs on macOS".into())
}
