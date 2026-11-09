//! The desktop's own folder picker. Blocks until the user chooses, so call
//! it off the UI thread. `Ok(None)` means they cancelled.
use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub fn folder(title: &str) -> Result<Option<PathBuf>, String> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    let err = |e: zbus::Error| format!("Couldn't open the folder picker: {e}");
    let conn = zbus::blocking::Connection::session().map_err(err)?;
    // The portal answers with a Response signal on a request object whose
    // path we can predict from our token; listen before asking so we can't
    // miss it.
    let token = format!("roku_remote_{}", std::process::id());
    let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let request = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", path.as_str(), "org.freedesktop.portal.Request")
        .map_err(err)?;
    let mut responses = request.receive_signal("Response").map_err(err)?;

    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("directory", Value::from(true));
    options.insert("modal", Value::from(true));
    conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.FileChooser"),
        "OpenFile",
        &("", title, options),
    )
    .map_err(err)?
    .body()
    .deserialize::<OwnedObjectPath>()
    .map_err(err)?;

    let msg = responses.next().ok_or("The folder picker closed unexpectedly")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(err)?;
    if code != 0 {
        return Ok(None);
    }
    let uris: Vec<String> = results.get("uris").and_then(|v| v.try_clone().ok()).and_then(|v| v.try_into().ok()).unwrap_or_default();
    Ok(uris.first().and_then(|u| u.strip_prefix("file://")).map(|p| PathBuf::from(crate::share::percent_decode(p))))
}

#[cfg(windows)]
pub fn folder(title: &str) -> Result<Option<PathBuf>, String> {
    Ok(rfd::FileDialog::new().set_title(title).pick_folder())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn folder(_title: &str) -> Result<Option<PathBuf>, String> {
    Err("No folder picker on this system".into())
}
