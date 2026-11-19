//! Everything that outlives the window: the network worker, the now-playing
//! poller and the state the tray and desktop media controls read.
use crate::ecp;
use crate::share::{self, Share};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum Cmd {
    Discover,
    Connect(String),
    Key(String, &'static str),
    Text(String, String),
    Launch(String, String),
    /// Play a YouTube video (by id) in the TV's YouTube channel.
    YouTube(String, String),
    /// Look for a title across every channel, in the TV's own search.
    Search(String, String),
    /// Wake a sleeping TV (by its network card's address), then connect.
    Wake(String, [u8; 6]),
}

pub enum Event {
    Found(Vec<ecp::Device>),
    Connected(ecp::Device, Vec<ecp::App>),
    Icon(String, String, Arc<[u8]>),
    NowPlaying(String, ecp::Playing),
    Error(String),
    /// A sleeping TV is being woken; `Connected` or `Error` follows.
    Waking,
    Ok,
}

/// Asks the main thread, which waits while the window is closed, to act.
pub enum Wake {
    Open,
    Quit,
}

#[derive(Default)]
pub struct Shared {
    /// The device keys and polls go to.
    pub device: Option<ecp::Device>,
    pub found: Vec<ecp::Device>,
    pub now: Option<ecp::Playing>,
    /// The open window's context; `None` while the window is closed.
    pub ctx: Option<egui::Context>,
    pub quit: bool,
    /// The shared folder, while sharing.
    pub share: Option<Arc<Share>>,
    /// A firewall that may keep the TV out of the share, checked when
    /// sharing starts.
    pub firewall: Option<Arc<share::Firewall>>,
    /// Whether the connected TV has Roku Media Player; `None` when its
    /// channel list is unavailable.
    pub media_player: Option<bool>,
    /// Sleep timer: when the TV is to be turned off, and the minutes chosen.
    pub sleep: Option<(Instant, u32)>,
    /// A newer release's version, once the update check has found one.
    pub update: Option<String>,
}

#[derive(Clone)]
pub struct Core {
    pub cmd: Sender<Cmd>,
    pub state: Arc<Mutex<Shared>>,
    wake: Sender<Wake>,
}

impl Core {
    /// Start the worker and poller. The window reads the events; the main
    /// thread waits on the wake-ups while the window is closed.
    pub fn start() -> (Self, Receiver<Event>, Receiver<Wake>) {
        let (cmd, cmd_rx) = channel();
        let (ev_tx, ev_rx) = channel();
        let (wake, wake_rx) = channel();
        let core = Self { cmd, state: Arc::default(), wake };
        let worker = core.clone();
        let tx = ev_tx.clone();
        std::thread::spawn(move || worker.work(cmd_rx, tx));
        let poller = core.clone();
        std::thread::spawn(move || poller.poll(ev_tx));
        if saved_updates() {
            let checker = core.clone();
            std::thread::spawn(move || {
                if let Some(version) = newer_release() {
                    checker.state.lock().unwrap().update = Some(version);
                    checker.repaint();
                }
            });
        }
        (core, ev_rx, wake_rx)
    }

    pub fn repaint(&self) {
        if let Some(ctx) = &self.state.lock().unwrap().ctx {
            ctx.request_repaint();
        }
    }

    /// Send a key to the current device, if any.
    pub fn key(&self, key: &'static str) {
        if let Some(dev) = &self.state.lock().unwrap().device {
            let _ = self.cmd.send(Cmd::Key(dev.host.clone(), key));
        }
    }

    pub fn window_open(&self) -> bool {
        self.state.lock().unwrap().ctx.is_some()
    }

    pub fn show_window(&self) {
        match self.state.lock().unwrap().ctx.clone() {
            Some(ctx) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            None => {
                let _ = self.wake.send(Wake::Open);
            }
        }
    }

    pub fn toggle_window(&self) {
        match self.state.lock().unwrap().ctx.clone() {
            Some(ctx) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            None => {
                let _ = self.wake.send(Wake::Open);
            }
        }
    }

    pub fn quit(&self) {
        let ctx = {
            let mut st = self.state.lock().unwrap();
            st.quit = true;
            st.ctx.clone()
        };
        match ctx {
            Some(ctx) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            None => {
                let _ = self.wake.send(Wake::Quit);
            }
        }
    }

    /// Turn the TV off in `minutes`; `None` cancels.
    pub fn set_sleep(&self, minutes: Option<u32>) {
        self.state.lock().unwrap().sleep = minutes.map(|m| (Instant::now() + Duration::from_secs(m as u64 * 60), m));
    }

    /// Share `folder` with Roku Media Player, replacing any earlier share,
    /// and open Media Player on the TV.
    pub fn start_share(&self, folder: &Path) -> Result<(), String> {
        // Stop the old share first so the new one can take its port.
        self.state.lock().unwrap().share = None;
        let share = Arc::new(Share::start(folder)?);
        save("share", &share.folder().to_string_lossy());
        let firewall = share::firewall().map(Arc::new);
        {
            let mut st = self.state.lock().unwrap();
            st.share = Some(share);
            st.firewall = firewall;
        }
        self.open_media_player();
        crate::refresh_tray();
        self.repaint();
        Ok(())
    }

    pub fn stop_share(&self) {
        self.state.lock().unwrap().share = None;
        crate::refresh_tray();
        self.repaint();
    }

    pub fn open_media_player(&self) {
        let st = self.state.lock().unwrap();
        if let Some(dev) = &st.device
            && st.media_player != Some(false)
        {
            let _ = self.cmd.send(Cmd::Launch(dev.host.clone(), share::MEDIA_PLAYER.into()));
        }
    }

    /// Network calls run on one worker thread so the window never blocks, and
    /// keypresses reach the TV in the order they were pressed. Channel logos
    /// load on their own thread so they never hold up a keypress.
    fn work(self, cmds: Receiver<Cmd>, ev_tx: Sender<Event>) {
        for cmd in cmds {
            let ev = match cmd {
                Cmd::Discover => {
                    let found = ecp::discover();
                    let mut st = self.state.lock().unwrap();
                    st.found = found.clone();
                    if let Some(dev) = st.device.clone()
                        && !st.found.iter().any(|d| d.host == dev.host)
                    {
                        st.found.push(dev);
                    }
                    drop(st);
                    crate::refresh_tray();
                    Event::Found(found)
                }
                Cmd::Connect(host) => self.connect(&host, &ev_tx),
                Cmd::Key(host, key) => match ecp::keypress(&host, key) {
                    Ok(()) => Event::Ok,
                    Err(e) => {
                        // Power pressed on a TV that has gone off the network: it's
                        // asleep, so wake it instead.
                        let mac = self.state.lock().unwrap().device.as_ref().filter(|d| d.host == host).and_then(|d| d.mac);
                        match mac.filter(|_| matches!(key, "PowerOn" | "PowerToggle") && ecp::device_info(&host).is_err()) {
                            Some(mac) => self.wake(&host, mac, &ev_tx),
                            None => Event::Error(e),
                        }
                    }
                },
                Cmd::Wake(host, mac) => self.wake(&host, mac, &ev_tx),
                Cmd::Search(host, text) => ecp::search(&host, &text).map_or_else(Event::Error, |_| Event::Ok),
                Cmd::Text(host, text) => ecp::type_text(&host, &text).map_or_else(Event::Error, |_| Event::Ok),
                Cmd::Launch(host, id) => ecp::launch(&host, &id).map_or_else(Event::Error, |_| Event::Ok),
                Cmd::YouTube(host, id) => ecp::play_youtube(&host, &id).map_or_else(Event::Error, |_| Event::Ok),
            };
            if ev_tx.send(ev).is_err() {
                break;
            }
            self.repaint();
        }
    }

    fn connect(&self, host: &str, ev_tx: &Sender<Event>) -> Event {
        let dev = match ecp::device_info(host) {
            Ok(dev) => dev,
            Err(e) => return Event::Error(e),
        };
        let apps = ecp::apps(host).unwrap_or_default();
        let (tx, core, ids) = (ev_tx.clone(), self.clone(), apps.iter().map(|a| a.id.clone()).collect::<Vec<_>>());
        let icon_host = host.to_string();
        std::thread::spawn(move || {
            for id in ids {
                if let Ok(bytes) = ecp::icon(&icon_host, &id) {
                    if tx.send(Event::Icon(icon_host.clone(), id, bytes.into())).is_err() {
                        return;
                    }
                    core.repaint();
                }
            }
        });
        save("host", &dev.host);
        if let Some(mac) = dev.mac {
            save("wake", &format!("{} {}", dev.host, mac.map(|b| format!("{b:02x}")).join(":")));
        }
        {
            let mut st = self.state.lock().unwrap();
            st.device = Some(dev.clone());
            st.now = None;
            st.media_player = (!apps.is_empty()).then(|| apps.iter().any(|a| a.id == share::MEDIA_PLAYER));
            if !st.found.iter().any(|d| d.host == dev.host) {
                st.found.push(dev.clone());
            }
        }
        crate::refresh_tray();
        Event::Connected(dev, apps)
    }

    /// Send wake-up packets until the TV answers (about half a minute at
    /// most), turn its screen on and connect.
    fn wake(&self, host: &str, mac: [u8; 6], ev_tx: &Sender<Event>) -> Event {
        let _ = ev_tx.send(Event::Waking);
        self.repaint();
        for _ in 0..8 {
            ecp::wake(mac);
            if ecp::device_info(host).is_ok() {
                let _ = ecp::keypress(host, "PowerOn");
                return self.connect(host, ev_tx);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        Event::Error("The TV didn't wake up. It may be unplugged, or set not to wake from the network.".into())
    }

    /// Asks the TV what's playing every second, for the now-playing bar, the
    /// tray and the desktop's media controls.
    fn poll(self, ev_tx: Sender<Event>) {
        let mut last: Option<(String, ecp::Playing)> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let asleep = {
                let mut st = self.state.lock().unwrap();
                let due = st.sleep.is_some_and(|(at, _)| Instant::now() >= at);
                if due {
                    st.sleep = None;
                }
                due
            };
            if asleep {
                self.key("PowerOff");
                self.repaint();
            }
            let Some(dev) = self.state.lock().unwrap().device.clone() else { continue };
            let Ok(now) = ecp::now_playing(&dev.host) else { continue };
            #[cfg(target_os = "linux")]
            crate::mpris::update(&dev.name, &now, crate::mpris::art_url(&dev.host, &now.app_id));
            if last.as_ref().is_some_and(|(h, n)| *h == dev.host && *n == now) {
                continue;
            }
            // The tray shows the channel and state, not the position, so it
            // only needs redrawing when those change.
            let tray_changed = last.as_ref().is_none_or(|(h, n)| *h != dev.host || n.app != now.app || n.state != now.state);
            last = Some((dev.host.clone(), now.clone()));
            self.state.lock().unwrap().now = Some(now.clone());
            if tray_changed {
                crate::refresh_tray();
            }
            // Nobody reads events while the window is closed; the window
            // picks up the state when it reopens.
            if self.window_open() {
                let _ = ev_tx.send(Event::NowPlaying(dev.host, now));
                self.repaint();
            }
        }
    }
}

fn config_path(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("APPDATA")?);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let dir = base.join("sofa-remote");
    // Settings saved under the app's old name move over once.
    let old = base.join("roku-remote");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(old, &dir);
    }
    Some(dir.join(name))
}

fn saved(name: &str) -> Option<String> {
    let value = std::fs::read_to_string(config_path(name)?).ok()?;
    Some(value.trim().to_string()).filter(|v| !v.is_empty())
}

fn save(name: &str, value: &str) {
    if let Some(path) = config_path(name) {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, value);
    }
}

pub fn saved_host() -> Option<String> {
    saved("host")
}

/// The theme and whether keys animate, as last chosen.
pub fn saved_appearance() -> (Option<String>, bool) {
    (saved("theme"), saved("animate").as_deref() != Some("off"))
}

pub fn save_appearance(theme: &str, animate: bool) {
    save("theme", theme);
    save("animate", if animate { "on" } else { "off" });
}

/// The TV connected last and its network card's address, for waking it.
pub fn saved_wake() -> Option<(String, [u8; 6])> {
    let line = saved("wake")?;
    let (host, mac) = line.split_once(' ')?;
    Some((host.to_string(), ecp::parse_mac(mac)?))
}

/// Whether to ask GitHub at startup for a newer release.
pub fn saved_updates() -> bool {
    saved("updates").as_deref() != Some("off")
}

pub fn save_updates(on: bool) {
    save("updates", if on { "on" } else { "off" });
}

/// The newest release's version, when it's newer than this build. Nothing
/// about this computer or the TV is sent.
fn newer_release() -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    let mut reply = agent
        .get("https://api.github.com/repos/billy1204/sofa-remote/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    let body = reply.body_mut().read_to_string().ok()?;
    let latest = body.split("\"tag_name\"").nth(1)?.split('"').nth(1)?.trim_start_matches('v');
    newer(latest, env!("CARGO_PKG_VERSION")).then(|| latest.to_string())
}

/// Whether version `a` (digits and dots only) is newer than `b`.
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| v.split('.').map(|n| n.parse::<u32>().ok()).collect::<Option<Vec<_>>>();
    matches!((parts(a), parts(b)), (Some(a), Some(b)) if a > b)
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn versions() {
        assert!(newer("0.11.0", "0.10.1"));
        assert!(newer("1.0", "0.99.9"));
        assert!(!newer("0.10.1", "0.10.1"));
        assert!(!newer("0.9.0", "0.10.1"));
        assert!(!newer("0.11.0-beta", "0.10.1"));
        assert!(!newer("<b>", "0.10.1"));
    }
}

/// Channels pinned to the top of the list, by id.
pub fn saved_favorites() -> Vec<String> {
    saved("favorites").map(|f| f.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

pub fn save_favorites(ids: &[String]) {
    save("favorites", &ids.join("\n"));
}

/// The folder shared last, if it still exists.
pub fn saved_share() -> Option<PathBuf> {
    saved("share").map(PathBuf::from).filter(|p| p.is_dir())
}
