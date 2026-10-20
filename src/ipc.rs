//! Line-based control socket. The daemon listens on a Unix socket; each
//! connection sends one command line and reads one response.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("STRIPLINE_SOCKET") {
        return PathBuf::from(p);
    }
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc_getuid() };
    std::env::temp_dir().join(format!("stripline-{uid}.sock"))
}

extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// Start the listener thread. `handle` runs on that thread and must forward
/// work to wherever it belongs.
pub fn serve<F>(handle: F) -> std::io::Result<()>
where
    F: Fn(String) -> String + Send + 'static,
{
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("another instance is already listening on {}", path.display()),
        ));
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new().name("ipc".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let reply = handle(line.trim().to_string());
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.write_all(b"\n");
        }
    })?;
    Ok(())
}

pub fn remove_socket() {
    let _ = std::fs::remove_file(socket_path());
}

/// Send one command to the running daemon and return its reply.
pub fn send(command: &str) -> Result<String, String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("cannot reach stripline at {} ({e}). Is it running?", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    stream.write_all(format!("{}\n", command.replace('\n', " ")).as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).map_err(|e| e.to_string())?;
    Ok(reply.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_over_socket() {
        let _env = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("stripline-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("STRIPLINE_SOCKET", dir.join("s.sock"));
        serve(|cmd| format!("got {cmd}")).unwrap();
        assert_eq!(send("focus-left").unwrap(), "got focus-left");
        assert_eq!(send("set-width 0.5").unwrap(), "got set-width 0.5");
        assert!(serve(|_| String::new()).is_err(), "second server refuses to start");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
