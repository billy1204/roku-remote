//! MPRIS media player on the session bus, so the desktop's media controls
//! (COSMIC's panel, GNOME, KDE) and the keyboard's media keys show and
//! control what's playing on the TV.
use crate::core::Core;
use crate::ecp::{Playing, State};
use std::collections::HashMap;
use std::sync::OnceLock;
use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";

/// Our own name on the bus: holding it makes this the running instance.
const APP_NAME: &str = "io.github.billy1204.RokuRemote";
const APP_PATH: &str = "/io/github/billy1204/RokuRemote";

static MPRIS: OnceLock<Mpris> = OnceLock::new();

/// Lets a second launch bring this instance's window up instead.
struct App {
    core: Core,
}

#[interface(name = "io.github.billy1204.RokuRemote")]
impl App {
    fn show(&self) {
        self.core.show_window();
    }
}

struct Root {
    core: Core,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.core.show_window();
    }
    fn quit(&self) {
        self.core.quit();
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Roku Remote"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "roku-remote"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    core: Core,
    now: Playing,
    device: String,
    art: Option<String>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play_pause(&self) {
        self.core.key("Play");
    }
    fn play(&self) {
        if self.now.state != State::Playing {
            self.core.key("Play");
        }
    }
    fn pause(&self) {
        if self.now.state == State::Playing {
            self.core.key("Play");
        }
    }
    fn next(&self) {
        self.core.key("Fwd");
    }
    /// Instant replay jumps back a few seconds and keeps playing, which is
    /// what "previous" is mostly wanted for on a TV.
    fn previous(&self) {
        self.core.key("InstantReplay");
    }
    fn stop(&self) {
        self.core.key("Back");
    }
    fn seek(&self, _offset: i64) {}
    fn set_position(&self, _track: ObjectPath<'_>, _position: i64) {}
    fn open_uri(&self, _uri: String) {}

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        match self.now.state {
            State::Playing => "Playing",
            State::Paused => "Paused",
            State::Stopped => "Stopped",
        }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut m = HashMap::new();
        let mut put = |k: &str, v: Value<'_>| {
            if let Ok(v) = OwnedValue::try_from(v) {
                m.insert(k.to_string(), v);
            }
        };
        let id: String = self.now.app_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let track = format!("/org/rokuremote/track/app{id}");
        put("mpris:trackid", Value::from(ObjectPath::try_from(track.as_str()).unwrap_or_else(|_| ObjectPath::from_static_str_unchecked("/org/rokuremote/track/none"))));
        if !self.now.app.is_empty() {
            put("xesam:title", Value::from(self.now.app.clone()));
        }
        if !self.device.is_empty() {
            put("xesam:artist", Value::from(vec![self.device.clone()]));
            put("xesam:album", Value::from(self.device.clone()));
        }
        if self.now.duration_ms > 0 {
            put("mpris:length", Value::from(self.now.duration_ms as i64 * 1000));
        }
        if let Some(art) = &self.art {
            put("mpris:artUrl", Value::from(art.clone()));
        }
        m
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.now.position_ms as i64 * 1000
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.now.state != State::Stopped
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.now.state != State::Stopped
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

pub struct Mpris {
    conn: zbus::blocking::Connection,
}

pub enum Start {
    Started,
    /// Another instance holds the name; it was asked to show its window.
    AlreadyRunning,
    /// No session bus: run without media controls.
    Unavailable,
}

/// Claim the app's bus name and publish the media player.
pub fn start(core: Core) -> Start {
    // zbus defaults to replacing (and being replaced by) other owners; a
    // second launch must find the name taken instead.
    let built = zbus::blocking::connection::Builder::session().and_then(|b| {
        b.allow_name_replacements(false)
            .replace_existing_names(false)
            .name(APP_NAME)?
            .serve_at(APP_PATH, App { core: core.clone() })?
            .serve_at(PATH, Root { core: core.clone() })?
            .serve_at(PATH, Player { core, now: Playing::default(), device: String::new(), art: None })?
            .build()
    });
    match built {
        Ok(conn) => {
            // MPRIS clients find players by this name prefix.
            let _ = conn.request_name_with_flags("org.mpris.MediaPlayer2.roku_remote", zbus::fdo::RequestNameFlags::DoNotQueue.into());
            let _ = MPRIS.set(Mpris { conn });
            Start::Started
        }
        Err(zbus::Error::NameTaken) => {
            let shown = zbus::blocking::Connection::session()
                .and_then(|c| c.call_method(Some(APP_NAME), APP_PATH, Some(APP_NAME), "Show", &()).map(drop));
            if shown.is_ok() { Start::AlreadyRunning } else { Start::Unavailable }
        }
        Err(_) => Start::Unavailable,
    }
}

pub fn update(device: &str, now: &Playing, art: Option<String>) {
    if let Some(m) = MPRIS.get() {
        m.update(device, now, art);
    }
}

impl Mpris {
    /// Publish the TV's state; signals only what changed.
    pub fn update(&self, device: &str, now: &Playing, art: Option<String>) {
        let Ok(iface) = self.conn.object_server().interface::<_, Player>(PATH) else { return };
        let mut p = iface.get_mut();
        let status = p.now.state != now.state;
        let meta = p.now.app_id != now.app_id || p.now.app != now.app || p.now.duration_ms != now.duration_ms || p.device != device || p.art != art;
        let caps = (p.now.state == State::Stopped) != (now.state == State::Stopped);
        // A jump of more than a few seconds from where playback would be is a seek.
        let seeked = now.state != State::Stopped && !meta && now.position_ms.abs_diff(p.now.position_ms) > 5000;
        p.now = now.clone();
        p.device = device.to_string();
        p.art = art;
        let emitter = iface.signal_emitter();
        async_io::block_on(async {
            if status {
                let _ = p.playback_status_changed(emitter).await;
            }
            if meta {
                let _ = p.metadata_changed(emitter).await;
            }
            if caps {
                let _ = p.can_go_next_changed(emitter).await;
                let _ = p.can_go_previous_changed(emitter).await;
            }
            if seeked || meta {
                let _ = emitter.emit("org.mpris.MediaPlayer2.Player", "Seeked", &(now.position_ms as i64 * 1000)).await;
            }
        });
    }
}

/// A PNG of the channel's logo in the cache directory, as a file:// URL for
/// `mpris:artUrl`. Fetched once per channel; desktops can't all read WebP.
pub fn art_url(host: &str, app_id: &str) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    let safe = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>();
    let path = base.join("roku-remote").join("art").join(format!("{}-{}.png", safe(host), safe(app_id)));
    if !path.exists() {
        let bytes = crate::ecp::icon(host, app_id).ok()?;
        let img = image::load_from_memory(&bytes).ok()?;
        std::fs::create_dir_all(path.parent()?).ok()?;
        img.save_with_format(&path, image::ImageFormat::Png).ok()?;
    }
    Some(format!("file://{}", path.display()))
}
