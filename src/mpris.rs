//! MPRIS media player on the session bus, so the desktop's media controls
//! (COSMIC's panel, GNOME, KDE) and the keyboard's media keys show and
//! control what's playing on the TV.
use crate::ecp::{Playing, State};
use eframe::egui;
use std::collections::HashMap;
use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";

struct Root {
    ctx: egui::Context,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    fn quit(&self) {
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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

/// Sends a key to the connected TV.
pub type KeySender = Box<dyn Fn(&'static str) + Send + Sync>;

struct Player {
    key: KeySender,
    now: Playing,
    device: String,
    art: Option<String>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play_pause(&self) {
        (self.key)("Play");
    }
    fn play(&self) {
        if self.now.state != State::Playing {
            (self.key)("Play");
        }
    }
    fn pause(&self) {
        if self.now.state == State::Playing {
            (self.key)("Play");
        }
    }
    fn next(&self) {
        (self.key)("Fwd");
    }
    /// Instant replay jumps back a few seconds and keeps playing, which is
    /// what "previous" is mostly wanted for on a TV.
    fn previous(&self) {
        (self.key)("InstantReplay");
    }
    fn stop(&self) {
        (self.key)("Back");
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

impl Mpris {
    pub fn start(ctx: egui::Context, key: KeySender) -> Option<Self> {
        let player = Player { key, now: Playing::default(), device: String::new(), art: None };
        let build = |name: String| {
            zbus::blocking::connection::Builder::session()?
                .name(name)?
                .serve_at(PATH, Root { ctx: ctx.clone() })?
                .build()
        };
        let conn = build("org.mpris.MediaPlayer2.roku_remote".into())
            .or_else(|_| build(format!("org.mpris.MediaPlayer2.roku_remote.instance{}", std::process::id())))
            .ok()?;
        conn.object_server().at(PATH, player).ok()?;
        Some(Self { conn })
    }

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
