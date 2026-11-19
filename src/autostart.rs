//! Start with the computer, hidden in the tray.

#[cfg(target_os = "linux")]
const ID: &str = "io.github.billy1204.sofa-remote";

#[cfg(target_os = "linux")]
fn sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// The desktop's autostart entry for this app.
#[cfg(target_os = "linux")]
fn entry() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join(format!("{ID}.desktop")))
}

#[cfg(target_os = "linux")]
pub fn enabled() -> bool {
    // A sandbox can't see the desktop's autostart folder; remember instead.
    if sandboxed() { crate::core::flag("autostart", false) } else { entry().is_some_and(|p| p.exists()) }
}

#[cfg(target_os = "linux")]
pub fn set(on: bool) -> Result<(), String> {
    if sandboxed() {
        // Flatpak apps ask the desktop to start them.
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("reason", Value::from("Keep the remote in the tray"));
        options.insert("autostart", Value::from(on));
        options.insert("commandline", Value::from(vec!["sofa-remote", "--hidden"]));
        zbus::blocking::Connection::session()
            .and_then(|c| {
                c.call_method(
                    Some("org.freedesktop.portal.Desktop"),
                    "/org/freedesktop/portal/desktop",
                    Some("org.freedesktop.portal.Background"),
                    "RequestBackground",
                    &("", options),
                )
            })
            .map_err(|e| format!("Couldn't change starting at login: {e}"))?;
        crate::core::set_flag("autostart", on);
        return Ok(());
    }
    let path = entry().ok_or("No settings folder")?;
    let err = |e: std::io::Error| format!("Couldn't change starting at login: {e}");
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(err(e)),
            _ => Ok(()),
        };
    }
    // An AppImage runs from a temporary mount; start the file itself.
    let exe = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok()).ok_or("Couldn't find the app's own file")?;
    let mut quoted = String::new();
    for c in exe.to_string_lossy().chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    let text = format!("[Desktop Entry]\nType=Application\nName=Sofa Remote\nExec=\"{quoted}\" --hidden\nIcon={ID}\nTerminal=false\n");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
    std::fs::write(path, text).map_err(err)
}

#[cfg(windows)]
const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("reg").args(args).creation_flags(CREATE_NO_WINDOW).output().is_ok_and(|o| o.status.success())
}

#[cfg(windows)]
pub fn enabled() -> bool {
    reg(&["query", RUN, "/v", "SofaRemote"])
}

#[cfg(windows)]
pub fn set(on: bool) -> Result<(), String> {
    let done = if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        reg(&["add", RUN, "/v", "SofaRemote", "/t", "REG_SZ", "/d", &format!("\"{}\" --hidden", exe.display()), "/f"])
    } else {
        reg(&["delete", RUN, "/v", "SofaRemote", "/f"]) || !enabled()
    };
    done.then_some(()).ok_or("Couldn't change starting at login".into())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn enabled() -> bool {
    false
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn set(_on: bool) -> Result<(), String> {
    Err("Starting at login isn't supported on this system".into())
}
