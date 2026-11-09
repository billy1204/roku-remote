//! Everything that outlives the window: the network worker, the now-playing
//! poller and the state the tray and desktop media controls read.
use crate::ecp;
use crate::share::{self, Share};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum Cmd {
    Discover,
    Connect(String),
    Key(String, &'static str),
    Text(String, String),
    Launch(String, String),
    /// Play a YouTube video (by id) in the TV's YouTube channel.
    YouTube(String, String),
}

pub enum Event {
    Found(Vec<ecp::Device>),
    Connected(ecp::Device, Vec<ecp::App>),
    Icon(String, String, Arc<[u8]>),
    NowPlaying(String, ecp::Playing),
    Error(String),
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
                Cmd::Connect(host) => match ecp::device_info(&host) {
                    Ok(dev) => {
                        let apps = ecp::apps(&host).unwrap_or_default();
                        let (tx, core, ids) = (ev_tx.clone(), self.clone(), apps.iter().map(|a| a.id.clone()).collect::<Vec<_>>());
                        let icon_host = host.clone();
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
                    Err(e) => Event::Error(e),
                },
                Cmd::Key(host, key) => ecp::keypress(&host, key).map_or_else(Event::Error, |_| Event::Ok),
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

    /// Asks the TV what's playing every second, for the now-playing bar, the
    /// tray and the desktop's media controls.
    fn poll(self, ev_tx: Sender<Event>) {
        let mut last: Option<(String, ecp::Playing)> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
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
    Some(base.join("roku-remote").join(name))
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

/// The folder shared last, if it still exists.
pub fn saved_share() -> Option<PathBuf> {
    saved("share").map(PathBuf::from).filter(|p| p.is_dir())
}
