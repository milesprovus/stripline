//! Settings window back end: a small HTTP server on 127.0.0.1 that serves the
//! settings page and a JSON API to read and write the config file. On macOS
//! the page is shown in a native window; it also works in any browser, which
//! is how it is developed and tested on other systems.
//!
//! The server only listens on loopback, checks the Host header (against DNS
//! rebinding) and requires a random per-run token on every API call, sent in
//! a custom header (which cross-site pages cannot add without a CORS
//! preflight this server never approves).

pub mod model;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::{self, Config};
use crate::{ipc, service};

const PAGE: &str = include_str!("ui.html");
const MAX_BODY: usize = 1 << 20;

static URL: OnceLock<String> = OnceLock::new();

fn random_token() -> String {
    let mut bytes = [0u8; 16];
    let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).is_ok();
    if !ok {
        // Fall back to time and address entropy; still unguessable enough
        // for a loopback-only server.
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let seed = t.as_nanos() ^ (&bytes as *const _ as u128) ^ (std::process::id() as u128) << 64;
        bytes = seed.to_le_bytes();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Start the server (once) and return the page URL, token included.
pub fn start() -> Result<String, String> {
    if let Some(url) = URL.get() {
        return Ok(url.clone());
    }
    let port: u16 = std::env::var("STRIPLINE_SETTINGS_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("settings server: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = random_token();
    let url = format!("http://127.0.0.1:{port}/?token={token}");
    std::thread::Builder::new()
        .name("settings".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let token = token.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, &token, port);
                });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(URL.get_or_init(|| url).clone())
}

struct Request {
    method: String,
    path: String,
    host: String,
    token: String,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let (mut host, mut token, mut len) = (String::new(), String::new(), 0usize);
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
                "host" => host = v,
                "x-stripline-token" => token = v,
                "content-length" => len = v.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if len > MAX_BODY {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, host, token, body })
}

fn respond(mut stream: TcpStream, status: &str, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; style-src 'self' \
         'unsafe-inline'; script-src 'self' 'unsafe-inline'; img-src 'self' data:\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn handle(stream: TcpStream, token: &str, port: u16) -> std::io::Result<()> {
    let req = read_request(&stream)?;
    let host_ok = req.host == format!("127.0.0.1:{port}") || req.host == format!("localhost:{port}");
    if !host_ok {
        return respond(stream, "403 Forbidden", "text/plain", b"forbidden");
    }
    let path = req.path.split('?').next().unwrap_or("/");
    if req.method == "GET" && path == "/" {
        return respond(stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes());
    }
    if !path.starts_with("/api/") {
        return respond(stream, "404 Not Found", "text/plain", b"not found");
    }
    if req.token != token {
        return respond(stream, "403 Forbidden", "text/plain", b"bad token");
    }
    let (status, value) = match api(&req.method, path, &req.body) {
        Ok(v) => ("200 OK", v),
        Err(e) => ("400 Bad Request", json!({ "ok": false, "error": e })),
    };
    respond(stream, status, "application/json", value.to_string().as_bytes())
}

/// Ask the running window manager something; `None` if it is not running.
fn daemon(cmd: &str) -> Option<String> {
    ipc::send(cmd).ok().filter(|r| !r.starts_with("error:"))
}

fn daemon_json(cmd: &str) -> Value {
    daemon(cmd).and_then(|r| serde_json::from_str(&r).ok()).unwrap_or(Value::Null)
}

#[derive(Deserialize)]
struct SaveRequest {
    config: Config,
    /// Every binding the user wants, as [combo, action].
    bindings: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct Flag {
    on: bool,
}

fn api(method: &str, path: &str, body: &[u8]) -> Result<Value, String> {
    let parse = |what| serde_json::from_slice::<Value>(body).map_err(|e| format!("bad {what}: {e}"));
    match (method, path) {
        ("GET", "/api/state") => Ok(state()),
        ("POST", "/api/save") => {
            let req: SaveRequest = serde_json::from_slice(body).map_err(|e| format!("bad settings: {e}"))?;
            save(req)
        }
        ("POST", "/api/recording") => {
            let on = serde_json::from_value::<Flag>(parse("request")?).map_err(|e| e.to_string())?.on;
            // While recording a shortcut, let every key reach the page.
            let _ = daemon(if on { "recording on" } else { "recording off" });
            Ok(json!({ "ok": true }))
        }
        ("POST", "/api/login-item") => {
            let on = serde_json::from_value::<Flag>(parse("request")?).map_err(|e| e.to_string())?.on;
            service::set_login_item(on).map(|m| json!({ "ok": true, "message": m, "login_item": service::installed() }))
        }
        ("GET", "/api/doctor") => Ok(json!({
            "ok": true,
            "report": daemon("doctor").unwrap_or_else(|| "Stripline is not running, so the checks cannot run.".into()),
        })),
        ("POST", "/api/open-config") => open_config(),
        _ => Err(format!("no such endpoint: {method} {path}")),
    }
}

/// The current file as the UI needs it, plus everything to build its controls.
fn state() -> Value {
    let path = config::config_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let (cfg, file_error) = match Config::parse(&text) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e)),
    };
    let running = daemon("ping").is_some();
    // Character keys as the active keyboard layout labels them.
    let layout = daemon_json("keylayout");
    let mut keys = serde_json::Map::new();
    for (code, us) in model::CHAR_KEY_CODES {
        let vk = crate::keys::ansi_code(*us).unwrap_or(0);
        let label = layout.get(vk.to_string()).and_then(Value::as_str).map(str::to_string).unwrap_or(us.to_string());
        keys.insert(code.to_string(), json!(label));
    }
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "path": path.display().to_string(),
        "file_error": file_error,
        "running": running,
        "config": cfg,
        "bindings": model::effective(&cfg),
        "defaults": Config::default(),
        "default_bindings": model::effective(&Config::default()),
        "catalog": model::catalog(),
        "displays": daemon_json("displays"),
        "apps": daemon_json("apps"),
        "keys": keys,
        "login_item": service::installed(),
    })
}

fn save(req: SaveRequest) -> Result<Value, String> {
    let mut cfg = req.config;
    cfg.bindings = model::overrides(&req.bindings, cfg.replace_default_bindings)?;
    cfg.validate()?;
    let text = model::to_toml(&cfg)?;
    let path = config::config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Keep the hand-written file once, before the first save from here.
    let backup = path.with_extension("toml.bak");
    if path.exists() && !backup.exists() {
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        if !old.starts_with(model::HEADER.lines().next().unwrap_or("#")) {
            std::fs::copy(&path, &backup).map_err(|e| format!("backup failed: {e}"))?;
        }
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    let message = match daemon("reload") {
        Some(m) => m,
        None => "saved (Stripline is not running; it will use these settings when it starts)".into(),
    };
    Ok(json!({ "ok": true, "message": message, "bindings": model::effective(&cfg) }))
}

fn open_config() -> Result<Value, String> {
    let path = config::config_path();
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, config::EXAMPLE).map_err(|e| e.to_string())?;
    }
    let (cmd, args): (&str, Vec<&std::ffi::OsStr>) = if cfg!(target_os = "macos") {
        ("open", vec!["-t".as_ref(), path.as_os_str()])
    } else {
        ("xdg-open", vec![path.as_os_str()])
    };
    std::process::Command::new(cmd).args(args).spawn().map_err(|e| format!("could not open the file: {e}"))?;
    Ok(json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(port: u16, raw: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_page_and_guards_the_api() {
        let _env = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("stripline-settings-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        std::env::set_var("STRIPLINE_SOCKET", dir.join("none.sock"));
        let url = start().unwrap();
        let port: u16 = url.split(':').nth(2).unwrap().split('/').next().unwrap().parse().unwrap();
        let token = url.split("token=").nth(1).unwrap();
        let host = format!("Host: 127.0.0.1:{port}\r\n");

        let page = http(port, &format!("GET / HTTP/1.1\r\n{host}\r\n"));
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("<html"));

        let bad_host = http(port, "GET /api/state HTTP/1.1\r\nHost: evil.example:80\r\n\r\n");
        assert!(bad_host.starts_with("HTTP/1.1 403"));
        let no_token = http(port, &format!("GET /api/state HTTP/1.1\r\n{host}\r\n"));
        assert!(no_token.starts_with("HTTP/1.1 403"));

        let state = http(port, &format!("GET /api/state HTTP/1.1\r\n{host}X-Stripline-Token: {token}\r\n\r\n"));
        assert!(state.starts_with("HTTP/1.1 200"), "{state}");
        let body = state.split("\r\n\r\n").nth(1).unwrap();
        let v: Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["running"], false);
        assert!(v["catalog"].as_array().unwrap().len() > 50);

        // Save a change and read it back from disk.
        let mut cfg = Config::default();
        cfg.layout.gap = 20.0;
        let bindings = model::effective(&cfg);
        let payload = json!({ "config": cfg, "bindings": bindings }).to_string();
        let saved = http(
            port,
            &format!(
                "POST /api/save HTTP/1.1\r\n{host}X-Stripline-Token: {token}\r\nContent-Length: {}\r\n\r\n{payload}",
                payload.len()
            ),
        );
        assert!(saved.starts_with("HTTP/1.1 200"), "{saved}");
        let on_disk = Config::load(&config::config_path()).unwrap();
        assert_eq!(on_disk.layout.gap, 20.0);

        // Invalid settings are refused and leave the file alone.
        cfg.border.color = "nope".into();
        let payload = json!({ "config": cfg, "bindings": bindings }).to_string();
        let refused = http(
            port,
            &format!(
                "POST /api/save HTTP/1.1\r\n{host}X-Stripline-Token: {token}\r\nContent-Length: {}\r\n\r\n{payload}",
                payload.len()
            ),
        );
        assert!(refused.starts_with("HTTP/1.1 400") && refused.contains("border.color"), "{refused}");
        assert_eq!(Config::load(&config::config_path()).unwrap().border.color, "#3D8BFF");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
