// No console window behind the remote on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod autostart;
mod cli;
mod core;
mod ecp;
mod look;
#[cfg(target_os = "linux")]
mod mpris;
mod pick;
mod routines;
mod share;
#[cfg(target_os = "linux")]
mod tray;

use eframe::egui::{self, Vec2, vec2};
use crate::core::{Cmd, Core, Event, Wake};
use look::Icon;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

/// Redraw the tray from the shared state (no-op without a tray).
pub fn refresh_tray() {
    #[cfg(target_os = "linux")]
    tray::refresh();
}

/// Lay out a row `total` wide, centered in the available width.
fn centered_row(ui: &mut egui::Ui, total: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
        add(ui);
    });
}

const REPO: &str = "https://github.com/billy1204/sofa-remote";
const FLASH_SECS: f64 = 0.15;
/// Keys that repeat while their button is held, after `HOLD_SECS`.
const REPEATS: [&str; 8] = ["Up", "Down", "Left", "Right", "VolumeUp", "VolumeDown", "ChannelUp", "ChannelDown"];
const HOLD_SECS: f64 = 0.5;
const REPEAT_SECS: f64 = 0.15;
/// Sleep timer choices, in minutes.
const SLEEP_MINUTES: [u32; 3] = [30, 60, 90];

/// A button being held down: when it next repeats, and whether it has yet.
struct Hold {
    key: &'static str,
    next: f64,
    repeated: bool,
}

struct Remote {
    core: Core,
    rx: Receiver<Event>,
    /// Window title last set, so it's only sent when it changes.
    title: String,
    host_input: String,
    show_ip: bool,
    device: Option<ecp::Device>,
    found: Vec<ecp::Device>,
    apps: Vec<ecp::App>,
    icons: HashMap<String, Arc<[u8]>>,
    now: Option<ecp::Playing>,
    text: String,
    youtube: String,
    status: String,
    error: bool,
    busy: bool,
    /// Key last sent from the keyboard and when, so its button lights up.
    flash: Option<(&'static str, f64)>,
    hold: Option<Hold>,
    /// Narrows the channel list to names containing it.
    filter: String,
    /// Channels pinned to the top of the list, by id.
    favorites: Vec<String>,
    /// The TV connected last, for the Wake TV button while it's unreachable.
    wake: Option<(String, [u8; 6])>,
    /// Whether to check for a newer release at startup.
    updates: bool,
    /// Whether the app starts with the computer.
    autostart: bool,
    /// The TV's volume and mute state, as last polled.
    audio: Option<ecp::Audio>,
    /// A volume picked on the slider and when to stop showing it in place
    /// of the TV's own: the TV steps there one press at a time.
    volume_set: Option<(u32, f64)>,
    routines: Vec<routines::Routine>,
    /// The routines are being edited rather than run.
    routines_edit: bool,
    /// The small always-on-top strip is showing instead of the full remote.
    mini: bool,
    /// The window's size before it shrank to the strip.
    full_size: Option<Vec2>,
    /// Live typing: each key goes to the TV as it's typed.
    live: bool,
    /// What live typing has sent to the TV so far.
    live_sent: String,
    /// The folder picker, while it's open.
    picking: Option<Receiver<Result<Option<PathBuf>, String>>>,
    share_help: bool,
    youtube_help: bool,
    connect_help: bool,
    appearance: bool,
    /// Dev aid: save a screenshot here once the window has settled, then quit.
    screenshot: Option<(PathBuf, u32)>,
}

/// A folder as people know it: `~/Videos` rather than `/home/me/Videos`.
fn folder_label(path: &Path) -> String {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rel) if rel.as_os_str().is_empty() => "~".into(),
        Some(rel) => format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display()),
        None => path.display().to_string(),
    }
}

impl Remote {
    fn new(core: Core, rx: Receiver<Event>) -> Self {
        let saved = core::saved_host();
        let mut app = Self {
            core,
            rx,
            title: String::new(),
            host_input: saved.clone().unwrap_or_default(),
            show_ip: false,
            device: None,
            found: Vec::new(),
            apps: Vec::new(),
            icons: HashMap::new(),
            now: None,
            text: String::new(),
            youtube: String::new(),
            status: String::new(),
            error: false,
            busy: false,
            flash: None,
            hold: None,
            filter: String::new(),
            favorites: core::saved_favorites(),
            wake: core::saved_wake(),
            updates: core::saved_updates(),
            autostart: autostart::enabled(),
            audio: None,
            volume_set: None,
            routines: core::saved_routines(),
            routines_edit: std::env::var_os("SOFA_REMOTE_ROUTINES").is_some(),
            mini: std::env::var_os("SOFA_REMOTE_MINI").is_some(),
            full_size: None,
            live: core::flag("live", false),
            live_sent: String::new(),
            picking: None,
            share_help: false,
            youtube_help: false,
            connect_help: false,
            appearance: std::env::var_os("SOFA_REMOTE_APPEARANCE").is_some(),
            screenshot: std::env::var_os("SOFA_REMOTE_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
        };
        // Reconnect before searching: the worker runs commands in order, so
        // when the search finishes the saved TV is already connected and a
        // search that hears from only one Roku doesn't switch to it.
        match saved {
            Some(host) => app.send(Cmd::Connect(host), "Connecting…"),
            None => {
                app.busy = true;
                app.status = "Searching for Roku devices…".into();
            }
        }
        // Search either way, so the device menu lists every Roku.
        let _ = app.core.cmd.send(Cmd::Discover);
        app
    }

    fn send(&mut self, cmd: Cmd, status: &str) {
        self.busy = matches!(cmd, Cmd::Discover | Cmd::Connect(_));
        self.status = status.to_string();
        self.error = false;
        let _ = self.core.cmd.send(cmd);
    }

    fn key(&mut self, key: &'static str) {
        self.core.key(key);
    }

    /// A button's click, or its repeats while it's held down.
    fn press(&mut self, ui: &egui::Ui, key: &'static str, clicked: bool, down: bool) {
        if down && REPEATS.contains(&key) {
            let now = ui.input(|i| i.time);
            match &mut self.hold {
                Some(h) if h.key == key => {
                    if now >= h.next {
                        self.core.key(key);
                        h.next = now + REPEAT_SECS;
                        h.repeated = true;
                    }
                }
                _ => self.hold = Some(Hold { key, next: now + HOLD_SECS, repeated: false }),
            }
            ui.ctx().request_repaint();
        } else if clicked {
            // Letting go after repeats isn't one more press.
            let repeated = self.hold.take().is_some_and(|h| h.key == key && h.repeated);
            if !repeated {
                self.key(key);
            }
        }
    }

    /// The window was reopened from the tray: catch up on what happened
    /// while it was closed.
    fn reopened(&mut self) {
        self.title.clear();
        self.mini = std::env::var_os("SOFA_REMOTE_MINI").is_some();
        self.now = self.core.state.lock().unwrap().now.clone();
    }

    fn lit(&self, ui: &egui::Ui, key: &str) -> bool {
        self.flash.is_some_and(|(k, t)| k == key && ui.input(|i| i.time) - t < FLASH_SECS)
    }

    /// Ask for a folder on another thread; the window keeps running.
    fn pick_folder(&mut self, ctx: &egui::Context) {
        if self.picking.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(pick::folder("Choose a folder to share with the TV"));
            ctx.request_repaint();
        });
        self.picking = Some(rx);
    }

    fn start_share(&mut self, folder: &Path) {
        match self.core.start_share(folder) {
            Ok(()) => self.status.clear(),
            Err(e) => {
                self.error = true;
                self.status = format!("Couldn't share the folder: {e}");
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        self.audio = self.core.state.lock().unwrap().audio;
        if let Some(rx) = &self.picking
            && let Ok(picked) = rx.try_recv()
        {
            self.picking = None;
            match picked {
                Ok(Some(folder)) => self.start_share(&folder),
                Ok(None) => {}
                Err(e) => {
                    self.error = true;
                    self.status = e;
                }
            }
        }
        let title = match (&self.device, &self.now) {
            (Some(dev), Some(now)) => format!("{} · {} — Sofa Remote", now.app, dev.name),
            _ => "Sofa Remote".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Found(devs) => {
                    self.busy = false;
                    if self.device.is_some() {
                        // A search from the refresh button, while connected.
                        if !self.error {
                            self.status.clear();
                        }
                    } else {
                        self.status = match devs.len() {
                            0 => "No Roku found. Choose “Enter IP address” in the device menu.".into(),
                            1 => String::new(),
                            _ => "Pick your TV from the device menu above.".into(),
                        };
                        if devs.len() == 1 {
                            let host = devs[0].host.clone();
                            self.send(Cmd::Connect(host), "Connecting…");
                        }
                    }
                    self.found = devs;
                }
                Event::Connected(dev, apps) => {
                    self.busy = false;
                    self.error = dev.limited;
                    self.status = if dev.limited {
                        "This TV only accepts volume from the network. On the TV, set Settings > \
                         System > Advanced system settings > Control by mobile apps to Enabled."
                            .into()
                    } else {
                        String::new()
                    };
                    self.host_input = dev.host.clone();
                    if let Some(mac) = dev.mac {
                        self.wake = Some((dev.host.clone(), mac));
                    }
                    self.show_ip = false;
                    if !self.found.iter().any(|d| d.host == dev.host) {
                        self.found.push(dev.clone());
                    }
                    self.device = Some(dev);
                    self.apps = apps;
                    self.icons.clear();
                    self.now = None;
                }
                Event::NowPlaying(host, now) => {
                    if self.device.as_ref().is_some_and(|d| d.host == host) {
                        self.now = Some(now);
                    }
                }
                Event::Icon(host, id, bytes) => {
                    if self.device.as_ref().is_some_and(|d| d.host == host) {
                        self.icons.insert(id, bytes);
                    }
                }
                Event::Error(e) => {
                    self.busy = false;
                    self.error = true;
                    self.status = e;
                }
                Event::Waking => {
                    self.busy = true;
                    self.error = false;
                    self.status = "Waking the TV…".into();
                }
                Event::Ok => {
                    if !self.busy && !self.error {
                        self.status.clear();
                    }
                }
            }
        }
    }

    fn shortcuts(&mut self, ui: &egui::Ui) {
        if self.device.is_none() || ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        use egui::Key as K;
        let map: &[(K, &'static str)] = &[
            (K::ArrowUp, "Up"),
            (K::ArrowDown, "Down"),
            (K::ArrowLeft, "Left"),
            (K::ArrowRight, "Right"),
            (K::Enter, "Select"),
            (K::Backspace, "Back"),
            (K::Escape, "Back"),
            (K::H, "Home"),
            (K::Space, "Play"),
            (K::Comma, "Rev"),
            (K::Period, "Fwd"),
            (K::R, "InstantReplay"),
            (K::I, "Info"),
            (K::Plus, "VolumeUp"),
            (K::Equals, "VolumeUp"),
            (K::Minus, "VolumeDown"),
            (K::M, "VolumeMute"),
            (K::PageUp, "ChannelUp"),
            (K::PageDown, "ChannelDown"),
        ];
        let (pressed, now): (Vec<&'static str>, f64) =
            ui.input(|i| (map.iter().filter(|(k, _)| i.key_pressed(*k)).map(|(_, v)| *v).collect(), i.time));
        for key in pressed {
            self.flash = Some((key, now));
            self.key(key);
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(FLASH_SECS));
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let name = self.device.as_ref().map_or("Sofa Remote".to_string(), |d| d.name.clone());
            let menu_w = ui.available_width() - 3.0 * (38.0 + ui.spacing().item_spacing.x);
            let mut pick = None;
            let combo = egui::ComboBox::from_id_salt("device")
                .selected_text(
                    egui::RichText::new(name).size(17.0).strong(),
                )
                .width(menu_w)
                .height(320.0)
                .show_ui(ui, |ui| {
                    ui.set_min_width(menu_w);
                    for d in &self.found {
                        let current = self.device.as_ref().is_some_and(|c| c.host == d.host);
                        let label = format!("{}   ·   {}", d.name, d.host);
                        if ui.selectable_label(current, label).clicked() {
                            pick = Some(d.host.clone());
                        }
                    }
                    if self.found.is_empty() {
                        ui.weak("No devices found yet");
                    }
                    ui.separator();
                    if ui.selectable_label(false, "Enter IP address…").clicked() {
                        self.show_ip = true;
                    }
                });
            combo.response.on_hover_text("Choose a Roku");
            if let Some(host) = pick {
                self.send(Cmd::Connect(host), "Connecting…");
            }
            let refresh = ui.add_enabled_ui(!self.busy, |ui| look::icon_key(ui, Vec2::splat(38.0), Icon::Refresh, false, None)).inner;
            if refresh.on_hover_text("Search the network again").clicked() {
                self.send(Cmd::Discover, "Searching for Roku devices…");
            }
            let help = look::text_key(ui, Vec2::splat(38.0), "?", self.connect_help).on_hover_text("TV not responding?");
            if help.clicked() {
                self.connect_help = !self.connect_help;
                self.appearance = false;
            }
            let look = look::icon_key(ui, Vec2::splat(38.0), Icon::Appearance, self.appearance, None).on_hover_text("Appearance");
            if look.clicked() {
                self.appearance = !self.appearance;
                self.connect_help = false;
            }
        });
        ui.add_space(-4.0);
        ui.horizontal(|ui| {
            let (dot, sub) = match &self.device {
                Some(d) if d.limited => (look::WARN, format!("{}  ·  volume only", d.host)),
                Some(d) => (look::GOOD, format!("{}  ·  connected", d.host)),
                None if self.busy => (look::pal().weak, "searching…".into()),
                None => (look::pal().weak, "not connected".into()),
            };
            ui.add_space(4.0);
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
            ui.label(egui::RichText::new(sub).small().color(look::pal().weak));
            if let Some(version) = self.core.state.lock().unwrap().update.clone() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    ui.hyperlink_to(egui::RichText::new(format!("Version {version} is available")).small(), format!("{REPO}/releases/latest"))
                        .on_hover_text("Open the download page");
                });
            }
        });
        if self.device.is_none()
            && !self.busy
            && let Some((host, mac)) = self.wake.clone()
        {
            let wake = ui.add(egui::Button::new("Wake TV").min_size(vec2(86.0, 34.0)));
            if wake.on_hover_text("Turn on the TV that was connected last, if it's asleep").clicked() {
                self.send(Cmd::Wake(host, mac), "Waking the TV…");
                self.busy = true;
            }
        }
        if self.show_ip || (self.device.is_none() && !self.busy && self.found.is_empty()) {
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.host_input)
                        .hint_text("IP address, e.g. 192.168.1.50")
                        .margin(egui::Margin::symmetric(10, 8))
                        .desired_width(ui.available_width() - 96.0),
                );
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let connect = ui.add_enabled(!self.busy, egui::Button::new("Connect").min_size(vec2(86.0, 34.0)));
                if (connect.clicked() || enter) && !self.host_input.trim().is_empty() {
                    let host = self.host_input.trim().to_string();
                    self.send(Cmd::Connect(host), "Connecting…");
                }
            });
        }
        if self.connect_help {
            self.connect_help(ui);
        }
        if self.appearance {
            appearance(ui, &mut self.updates);
            self.startup(ui);
        }
    }

    /// The start-at-login switch, under Appearance.
    fn startup(&mut self, ui: &mut egui::Ui) {
        let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
        egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label("Start at login");
                    ui.label(weak("Opens with the computer, tucked in the tray"));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = self.autostart;
                    look::toggle(ui, &mut on).on_hover_text(if on { "On" } else { "Off" });
                    if on != self.autostart {
                        match autostart::set(on) {
                            Ok(()) => self.autostart = on,
                            Err(e) => {
                                self.error = true;
                                self.status = e;
                            }
                        }
                    }
                });
            });
        });
    }

    fn connect_help(&self, ui: &mut egui::Ui) {
        let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
        ui.add_space(6.0);
        egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            if let Some(dev) = &self.device {
                ui.label(egui::RichText::new(&dev.name).strong());
                egui::Grid::new("tv-info").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                    for (label, value) in dev.info.iter().map(|(l, v)| (*l, v.as_str())).chain([("Address", dev.host.as_str())]) {
                        ui.label(weak(label));
                        ui.label(egui::RichText::new(value).small());
                        ui.end_row();
                    }
                });
                ui.separator();
            }
            ui.label(egui::RichText::new("TV not responding?").strong());
            ui.add(
                egui::Label::new(weak(
                    "Newer Roku TVs only accept volume from apps until “Control by mobile apps” is enabled. \
                     Using the TV's own remote:",
                ))
                .wrap(),
            );
            for (n, step) in [
                ("1", "Press Home and open Settings"),
                ("2", "System"),
                ("3", "Advanced system settings"),
                ("4", "Control by mobile apps"),
                ("5", "Network access: choose Enabled"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(n).strong().color(look::pal().accent));
                    ui.label(egui::RichText::new(step).small());
                });
            }
            if self.device.as_ref().is_some_and(|d| d.limited) {
                ui.add(egui::Label::new(egui::RichText::new("This TV is set to Limited right now, so only volume works.").small().color(look::WARN)).wrap());
            }
            ui.add(
                egui::Label::new(weak(
                    "Still nothing? Make sure the TV and this computer are on the same network, then press the \
                     search button to look again.",
                ))
                .wrap(),
            );
        });
    }

    /// One key: sends `key` on click, lights up while pressed or flashed.
    fn icon_key(&mut self, ui: &mut egui::Ui, size: Vec2, icon: Icon, key: &'static str, tip: &str) {
        let tint = (key == "PowerToggle").then_some(look::POWER);
        let lit = self.lit(ui, key) || (key == "VolumeMute" && self.audio.is_some_and(|a| a.muted));
        let resp = look::icon_key(ui, size, icon, lit, tint).on_hover_text(tip);
        self.press(ui, key, resp.clicked(), resp.is_pointer_button_down_on());
    }

    fn text_key(&mut self, ui: &mut egui::Ui, size: Vec2, label: &str, key: &'static str, tip: &str) {
        let lit = self.lit(ui, key);
        let resp = look::text_key(ui, size, label, lit).on_hover_text(tip);
        self.press(ui, key, resp.clicked(), resp.is_pointer_button_down_on());
    }

    /// The TV's volume, and a slider that steps it to a chosen level.
    fn volume(&mut self, ui: &mut egui::Ui, width: f32) {
        let (Some(audio), Some(host)) = (self.audio, self.device.as_ref().map(|d| d.host.clone())) else { return };
        let now = ui.input(|i| i.time);
        if self.volume_set.is_some_and(|(v, until)| now > until || (until.is_finite() && v == audio.volume)) {
            self.volume_set = None;
        }
        let mut level = self.volume_set.map_or(audio.volume, |(v, _)| v);
        let line = if audio.muted { format!("Muted  ·  volume {level}") } else { format!("Volume {level}") };
        ui.vertical_centered(|ui| ui.label(egui::RichText::new(line).small().color(look::pal().weak)));
        centered_row(ui, width, |ui| {
            ui.spacing_mut().slider_width = width;
            let slider = ui.add(egui::Slider::new(&mut level, 0..=100).show_value(false)).on_hover_text("Drag to set the volume");
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                let _ = self.core.cmd.send(Cmd::Volume(host, level));
                // Long enough for the TV to step from one end to the other.
                self.volume_set = Some((level, now + 20.0));
            } else if slider.changed() {
                self.volume_set = Some((level, f64::INFINITY));
            }
        });
    }

    /// Turn the TV off after a while; pressing the lit choice cancels.
    fn sleep_timer(&mut self, ui: &mut egui::Ui) {
        let sleep = self.core.state.lock().unwrap().sleep;
        let line = match self.core.sleep_left() {
            Some(left) => {
                // Count the minutes down without waiting for input.
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(5));
                format!("TV turns off in {left} min")
            }
            None => "Sleep timer".into(),
        };
        ui.vertical_centered(|ui| ui.label(egui::RichText::new(line).small().color(look::pal().weak)));
        let (kw, gap) = (74.0, ui.spacing().item_spacing.x);
        centered_row(ui, 3.0 * kw + 2.0 * gap, |ui| {
            for minutes in SLEEP_MINUTES {
                let on = sleep.is_some_and(|(_, m)| m == minutes);
                let tip = if on { "Cancel the sleep timer".to_string() } else { format!("Turn the TV off in {minutes} minutes") };
                if look::text_key(ui, vec2(kw, 30.0), &format!("{minutes} min"), on).on_hover_text(tip).clicked() {
                    self.core.set_sleep((!on).then_some(minutes));
                }
            }
        });
    }

    fn remote(&mut self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let gap = ui.spacing().item_spacing.x;
        let is_tv = self.device.as_ref().is_some_and(|d| d.is_tv);
        let small = Vec2::splat(52.0);

        // Top row: Back · Home · Power
        let top: &[(Icon, &'static str, &str)] = if is_tv {
            &[(Icon::Back, "Back", "Back  (Backspace)"), (Icon::Home, "Home", "Home  (H)"), (Icon::Power, "PowerToggle", "Power")]
        } else {
            &[(Icon::Back, "Back", "Back  (Backspace)"), (Icon::Home, "Home", "Home  (H)")]
        };
        let spread = 34.0;
        centered_row(ui, top.len() as f32 * small.x + (top.len() - 1) as f32 * (gap + spread), |ui| {
            for (i, (icon, key, tip)) in top.iter().enumerate() {
                if i > 0 {
                    ui.add_space(spread);
                }
                self.icon_key(ui, small, *icon, key, tip);
            }
        });

        ui.add_space(6.0);
        let d = (w - 40.0).clamp(180.0, 250.0);
        let lit = self.flash.filter(|_| ["Up", "Down", "Left", "Right", "Select"].iter().any(|k| self.lit(ui, k))).map(|(k, _)| k);
        let mut pad = (None, None);
        ui.vertical_centered(|ui| pad = look::dpad(ui, d, lit));
        if let Some(key) = pad.0.or(pad.1) {
            self.press(ui, key, pad.0.is_some(), pad.1.is_some());
        }
        ui.add_space(6.0);

        // Replay · Options
        centered_row(ui, 2.0 * small.x + gap + d - small.x * 2.0, |ui| {
            self.icon_key(ui, small, Icon::Replay, "InstantReplay", "Instant replay  (R)");
            ui.add_space(d - small.x * 2.0 - gap);
            self.icon_key(ui, small, Icon::Options, "Info", "Options  (I)");
        });

        // Transport
        let tk = vec2(64.0, 46.0);
        centered_row(ui, 3.0 * tk.x + 2.0 * gap, |ui| {
            self.icon_key(ui, tk, Icon::Rev, "Rev", "Rewind  ( , )");
            self.icon_key(ui, tk, Icon::Play, "Play", "Play / Pause  (Space)");
            self.icon_key(ui, tk, Icon::Fwd, "Fwd", "Fast forward  ( . )");
        });

        if is_tv {
            ui.add_space(4.0);
            // Volume rocker: one pill split in three.
            let vk = vec2(64.0, 40.0);
            centered_row(ui, 3.0 * vk.x + 2.0 * gap, |ui| {
                self.icon_key(ui, vk, Icon::VolDown, "VolumeDown", "Volume down  (−)");
                self.icon_key(ui, vk, Icon::Mute, "VolumeMute", "Mute  (M)");
                self.icon_key(ui, vk, Icon::VolUp, "VolumeUp", "Volume up  (+)");
            });
            self.volume(ui, 3.0 * vk.x + 2.0 * gap);
            // Channel rocker, for the antenna.
            centered_row(ui, 2.0 * vk.x + gap, |ui| {
                self.text_key(ui, vec2(vk.x, 30.0), "CH −", "ChannelDown", "Channel down  (Page Down)");
                self.text_key(ui, vec2(vk.x, 30.0), "CH +", "ChannelUp", "Channel up  (Page Up)");
            });
            let inputs = [
                [("HDMI 1", "InputHDMI1"), ("HDMI 2", "InputHDMI2"), ("HDMI 3", "InputHDMI3")],
                [("HDMI 4", "InputHDMI4"), ("AV", "InputAV1"), ("Antenna", "InputTuner")],
            ];
            let iw = ((w - 2.0 * gap) / 3.0).min(74.0);
            for row in inputs {
                centered_row(ui, 3.0 * iw + 2.0 * gap, |ui| {
                    for (label, key) in row {
                        self.text_key(ui, vec2(iw, 30.0), label, key, "Switch input");
                    }
                });
            }
        }
        let mut extras: Vec<(&str, &'static str, &str)> = Vec::new();
        if is_tv {
            extras.extend([("TV on", "PowerOn", "Turn the TV on"), ("TV off", "PowerOff", "Turn the TV off")]);
        }
        if self.device.as_ref().is_some_and(|d| d.find_remote) {
            extras.push(("Find remote", "FindRemote", "Make the Roku remote beep so you can find it"));
        }
        if !extras.is_empty() {
            let n = extras.len() as f32;
            let ew = ((w - (n - 1.0) * gap) / n).min(92.0);
            centered_row(ui, n * ew + (n - 1.0) * gap, |ui| {
                for (label, key, tip) in extras {
                    self.text_key(ui, vec2(ew, 30.0), label, key, tip);
                }
            });
        }
        if is_tv {
            ui.add_space(4.0);
            self.sleep_timer(ui);
        }
    }

    fn now_playing(&mut self, ui: &mut egui::Ui) {
        let Some(now) = self.now.clone() else { return };
        fn clock(ms: u64) -> String {
            let s = ms / 1000;
            if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
        }
        egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (art, _) = ui.allocate_exact_size(vec2(64.0, 48.0), egui::Sense::hover());
                ui.painter().rect_filled(art, 8.0, look::pal().key);
                match self.icons.get(&now.app_id) {
                    Some(bytes) => {
                        egui::Image::from_bytes(format!("bytes://channel/{}.img", now.app_id), egui::load::Bytes::Shared(bytes.clone()))
                            .corner_radius(8)
                            .paint_at(ui, art);
                    }
                    None => look::paint_icon(ui.painter(), art.shrink(12.0), Icon::Home, look::pal().weak),
                }
                let play_w = 40.0;
                let text_w = ui.available_width() - play_w - ui.spacing().item_spacing.x;
                ui.vertical(|ui| {
                    ui.set_width(text_w);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.add(egui::Label::new(egui::RichText::new(&now.app).strong()).truncate());
                    let state = match now.state {
                        ecp::State::Playing => "Playing",
                        ecp::State::Paused => "Paused",
                        ecp::State::Stopped => "Open",
                    };
                    let line = if now.duration_ms > 0 {
                        format!("{state}  ·  {} / {}", clock(now.position_ms), clock(now.duration_ms))
                    } else if now.position_ms > 0 {
                        format!("{state}  ·  {}", clock(now.position_ms))
                    } else {
                        state.to_string()
                    };
                    ui.label(egui::RichText::new(line).small().color(look::pal().weak));
                    if now.duration_ms > 0 {
                        let (bar, _) = ui.allocate_exact_size(vec2(text_w, 4.0), egui::Sense::hover());
                        ui.painter().rect_filled(bar, 2.0, look::pal().key);
                        let done = (now.position_ms as f32 / now.duration_ms as f32).clamp(0.0, 1.0);
                        let mut fill = bar;
                        fill.set_width(bar.width() * done);
                        ui.painter().rect_filled(fill, 2.0, look::pal().accent);
                    }
                });
                if now.state != ecp::State::Stopped {
                    let icon = if now.state == ecp::State::Playing { Icon::Pause } else { Icon::PlayOnly };
                    let lit = self.lit(ui, "Play");
                    if look::icon_key(ui, Vec2::splat(play_w), icon, lit, None).on_hover_text("Play / Pause").clicked() {
                        self.key("Play");
                    }
                }
            });
        });
    }

    fn search(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .hint_text("Type or search the TV…")
                    .margin(egui::Margin::symmetric(12, 9))
                    .desired_width(ui.available_width() - 2.0 * (44.0 + ui.spacing().item_spacing.x)),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let send = look::icon_key(ui, Vec2::splat(38.0), Icon::Send, false, Some(look::pal().accent)).on_hover_text(if self.live {
                "Press Enter on the TV's keyboard  (Enter)"
            } else {
                "Type this into a search box that's open on the TV  (Enter)"
            });
            if self.live
                && let Some(host) = self.device.as_ref().map(|d| d.host.clone())
            {
                self.live_type(&host);
                if send.clicked() || enter {
                    let _ = self.core.cmd.send(Cmd::Key(host, "Enter"));
                    self.text.clear();
                    self.live_sent.clear();
                }
            }
            let find = look::icon_key(ui, Vec2::splat(38.0), Icon::Search, false, Some(look::pal().accent))
                .on_hover_text("Search every channel on the TV for this");
            if find.clicked()
                && !self.text.trim().is_empty()
                && let Some(dev) = &self.device
            {
                let _ = self.core.cmd.send(Cmd::Search(dev.host.clone(), self.text.trim().to_string()));
                self.text.clear();
                self.live_sent.clear();
                self.error = false;
                self.status = "Opening search on the TV…".into();
            }
            if (send.clicked() || enter)
                && !self.live
                && !self.text.is_empty()
                && let Some(dev) = &self.device
            {
                let _ = self.core.cmd.send(Cmd::Text(dev.host.clone(), std::mem::take(&mut self.text)));
            }
        });
        let live = ui.checkbox(&mut self.live, egui::RichText::new("Send each key as I type").small().color(look::pal().weak));
        if live.on_hover_text("Letters and Backspace go to the TV's on-screen keyboard right away").changed() {
            core::set_flag("live", self.live);
            // Start from what's in the box; it isn't on the TV yet.
            self.live_sent = self.text.clone();
        }
    }

    /// Send the TV the difference between what's typed and what it has.
    fn live_type(&mut self, host: &str) {
        if self.text == self.live_sent {
            return;
        }
        let same = self.live_sent.chars().zip(self.text.chars()).take_while(|(a, b)| a == b).count();
        for _ in same..self.live_sent.chars().count() {
            let _ = self.core.cmd.send(Cmd::Key(host.to_string(), "Backspace"));
        }
        let added: String = self.text.chars().skip(same).collect();
        if !added.is_empty() {
            let _ = self.core.cmd.send(Cmd::Text(host.to_string(), added));
        }
        self.live_sent = self.text.clone();
    }

    fn youtube(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("YOUTUBE").small().strong().color(look::pal().weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let help = look::text_key(ui, vec2(26.0, 22.0), "?", self.youtube_help).on_hover_text("How to paste a link");
                if help.clicked() {
                    self.youtube_help = !self.youtube_help;
                }
            });
        });
        if self.youtube_help {
            let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
            egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(egui::RichText::new("Pasting a link").strong());
                ui.add(
                    egui::Label::new(weak(
                        "Copy the video's link (in a browser, from the address bar; in the YouTube app, Share > Copy link). \
                         Then click the box below and press Ctrl+V to paste it.",
                    ))
                    .wrap(),
                );
                ui.add(egui::Label::new(egui::RichText::new("Right-click Paste doesn't work in this box. Use Ctrl+V.").small().color(look::WARN)).wrap());
                ui.add(egui::Label::new(weak("Then press Enter or the play button, and the video plays in YouTube on the TV.")).wrap());
            });
        }
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.youtube)
                    .hint_text("Paste a YouTube link (Ctrl+V)…")
                    .margin(egui::Margin::symmetric(12, 9))
                    .desired_width(ui.available_width() - 44.0 - ui.spacing().item_spacing.x),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let play = look::icon_key(ui, Vec2::splat(38.0), Icon::PlayOnly, false, Some(look::pal().accent)).on_hover_text("Play on the TV");
            if (play.clicked() || enter) && !self.youtube.trim().is_empty() {
                self.play_youtube();
            }
        });
    }

    fn play_youtube(&mut self) {
        let Some(dev) = &self.device else { return };
        self.error = true;
        let Some(id) = ecp::youtube_id(&self.youtube) else {
            self.status = "That isn't a YouTube video link. Copy the link from the video's Share button.".into();
            return;
        };
        if !self.apps.is_empty() && !self.apps.iter().any(|a| a.id == ecp::YOUTUBE) {
            self.status = "YouTube isn't installed on this TV. Add it from the TV's Streaming Store.".into();
            return;
        }
        let _ = self.core.cmd.send(Cmd::YouTube(dev.host.clone(), id));
        self.error = false;
        self.status = "Opening the video on YouTube…".into();
        self.youtube.clear();
    }

    fn share(&mut self, ui: &mut egui::Ui) {
        let (share, firewall, media_player, seen) = {
            let st = self.core.state.lock().unwrap();
            let host = self.device.as_ref().map(|d| d.host.as_str()).unwrap_or_default();
            let seen = st.share.as_ref().and_then(|s| s.last_seen(host));
            (st.share.clone(), st.firewall.clone(), st.media_player, seen)
        };
        let small = |t: &str| egui::RichText::new(t).small();
        let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
        let warn = |t: &str| egui::RichText::new(t).small().color(look::WARN);
        let button = |t: &str| egui::Button::new(t).min_size(vec2(0.0, 32.0));

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("SHARE MEDIA").small().strong().color(look::pal().weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let help = look::text_key(ui, vec2(26.0, 22.0), "?", self.share_help)
                    .on_hover_text("How sharing works, and what to do if the TV can't see this computer");
                if help.clicked() {
                    self.share_help = !self.share_help;
                }
            });
        });
        egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            match &share {
                None => {
                    ui.add(egui::Label::new(weak("Play videos, music and photos from this computer on the TV.")).wrap());
                    ui.horizontal(|ui| {
                        let picking = self.picking.is_some();
                        if ui.add_enabled(!picking, button(if picking { "Choosing…" } else { "Share a folder…" })).clicked() {
                            self.pick_folder(ui.ctx());
                        }
                        if let Some(last) = core::saved_share() {
                            let name = last.file_name().map_or_else(|| folder_label(&last), |n| n.to_string_lossy().into_owned());
                            if ui.add(button(&format!("Share “{name}”"))).on_hover_text(folder_label(&last)).clicked() {
                                self.start_share(&last);
                            }
                        }
                    });
                }
                Some(share) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new(format!("Sharing {}", folder_label(share.folder()))).strong()).truncate());
                    });
                    ui.horizontal(|ui| {
                        if ui.add(button("Open on TV")).on_hover_text("Open Roku Media Player on the TV").clicked() {
                            self.core.open_media_player();
                        }
                        if ui.add_enabled(self.picking.is_none(), button("Change…")).clicked() {
                            self.pick_folder(ui.ctx());
                        }
                        if ui.add(button("Stop")).clicked() {
                            self.core.stop_share();
                        }
                    });
                    let name = share::server_name();
                    let (dot, line) = match seen {
                        Some(_) => (look::GOOD, "The TV is connected to the share".to_string()),
                        None => (look::pal().weak, format!("On the TV, open Roku Media Player and choose “{name}”")),
                    };
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, dot);
                        ui.add(egui::Label::new(small(&line)).wrap());
                    });
                    if seen.is_none() {
                        // Notice the TV connecting without waiting for input.
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                        if let Some(fw) = &firewall {
                            ui.add(egui::Label::new(warn(&format!("{} is on and may stop the TV from seeing this computer. Click ? for the fix.", fw.name))).wrap());
                        }
                        if !share.announced {
                            ui.add(egui::Label::new(warn("Another program is using the network discovery port, so the TV may not find this computer.")).wrap());
                        }
                    }
                }
            }
            if media_player == Some(false) {
                ui.add(egui::Label::new(warn("Roku Media Player isn't installed on this TV. Click ? to see how to get it.")).wrap());
            }
        });

        if self.share_help {
            self.share_help(ui, firewall.as_deref(), media_player, seen.is_some());
        }
    }

    fn share_help(&mut self, ui: &mut egui::Ui, firewall: Option<&share::Firewall>, media_player: Option<bool>, seen: bool) {
        let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
        egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            let step = |ui: &mut egui::Ui, n: &str, title: &str, body: &str| {
                ui.horizontal_top(|ui| {
                    ui.label(egui::RichText::new(n).strong().color(look::pal().accent));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(title).strong());
                        if !body.is_empty() {
                            ui.add(egui::Label::new(weak(body)).wrap());
                        }
                    });
                });
            };

            let (badge, color) = match media_player {
                Some(true) => ("Installed on this TV ✓", look::GOOD),
                Some(false) => ("Not installed on this TV", look::WARN),
                None => ("Couldn't check this TV", look::pal().weak),
            };
            step(
                ui,
                "1",
                "Get Roku Media Player on the TV",
                "It's free, made by Roku. On the TV press Home, open Streaming Store (or Search), find                  Roku Media Player and choose Add channel.",
            );
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.label(egui::RichText::new(badge).small().color(color));
            });
            step(ui, "2", "Share a folder", "Click Share a folder… and pick the folder with your videos, music or photos.");
            step(
                ui,
                "3",
                "Play it on the TV",
                &format!(
                    "Roku Media Player opens on the TV. Choose All or Media Servers, then “{}”, and                      browse with the Roku remote.",
                    share::server_name()
                ),
            );
            ui.add(egui::Label::new(weak("Plays MP4, MKV and MOV video, MP3, AAC, FLAC and WAV music, and JPG, PNG and GIF photos.")).wrap());

            ui.separator();
            ui.label(egui::RichText::new("If the TV can't see this computer").strong());
            ui.add(egui::Label::new(weak("Both must be on the same network. A firewall on this computer can also keep the TV out.")).wrap());
            match firewall {
                _ if seen => {
                    ui.label(egui::RichText::new("The TV has connected, so nothing is blocking it ✓").small().color(look::GOOD));
                }
                Some(fw) if fw.fix.starts_with("sudo") => {
                    ui.add(egui::Label::new(egui::RichText::new(format!("{} is on. To let the TV in, run in a terminal:", fw.name)).small().color(look::WARN)).wrap());
                    let mut fix = fw.fix.clone();
                    ui.add(egui::TextEdit::multiline(&mut fix).code_editor().desired_rows(2).desired_width(f32::INFINITY));
                    if ui.small_button("Copy commands").clicked() {
                        ui.ctx().copy_text(fw.fix.clone());
                    }
                }
                Some(fw) => {
                    ui.add(egui::Label::new(egui::RichText::new(format!("{} is on. {}", fw.name, fw.fix)).small().color(look::WARN)).wrap());
                }
                None if !share::firewall_checkable() => {
                    ui.add(egui::Label::new(weak("The Flatpak version can't check this computer's firewall. If one is on, let in TCP ports 8200–8209 and UDP port 1900.")).wrap());
                }
                None => {
                    ui.label(egui::RichText::new("No firewall found that would block the TV.").small().color(look::GOOD));
                }
            }
        });
    }

    fn remote_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(look::pal().body)
            .corner_radius(28)
            .inner_margin(egui::Margin::symmetric(14, 20))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.remote(ui);
            });
    }

    /// Search, YouTube and sharing, which sit together in every layout.
    fn tools(&mut self, ui: &mut egui::Ui) {
        self.search(ui);
        ui.add_space(4.0);
        self.youtube(ui);
        ui.add_space(4.0);
        self.share(ui);
        ui.add_space(4.0);
        self.routines(ui);
    }

    /// Routines: a button each to run them, or the editor.
    fn routines(&mut self, ui: &mut egui::Ui) {
        let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("ROUTINES").small().strong().color(look::pal().weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.routines_edit { "Done" } else { "Edit" };
                if look::text_key(ui, vec2(48.0, 22.0), label, self.routines_edit).on_hover_text("Make or change routines").clicked() {
                    self.routines_edit = !self.routines_edit;
                }
            });
        });
        if !self.routines_edit {
            if self.routines.is_empty() {
                ui.add(egui::Label::new(weak("Several steps in one click, like TV on, HDMI 1, volume 20. Click Edit to make one.")).wrap());
                return;
            }
            let mut run = None;
            ui.horizontal_wrapped(|ui| {
                for r in &self.routines {
                    let steps = r.steps.iter().map(|s| s.label()).collect::<Vec<_>>().join(", ");
                    if ui.add(egui::Button::new(&r.name).min_size(vec2(0.0, 32.0))).on_hover_text(steps).clicked() {
                        run = Some(r.clone());
                    }
                }
            });
            if let Some(r) = run {
                self.core.routine(r.steps);
                self.error = false;
                self.status = format!("Running “{}”…", r.name);
            }
            return;
        }
        let before = self.routines.clone();
        let apps = self.apps.clone();
        let mut delete = None;
        for (i, r) in self.routines.iter_mut().enumerate() {
            egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut r.name).hint_text("Name").desired_width(ui.available_width() - 70.0));
                    if ui.small_button("Delete").on_hover_text("Delete this routine").clicked() {
                        delete = Some(i);
                    }
                });
                let mut remove = None;
                for (j, step) in r.steps.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(weak(&format!("{}.", j + 1)));
                        match step {
                            routines::Step::Volume(v) => {
                                ui.label("Volume");
                                ui.add(egui::DragValue::new(v).range(0..=100));
                            }
                            routines::Step::Sleep(m) => {
                                ui.label("Sleep timer");
                                ui.add(egui::DragValue::new(m).range(5..=240).suffix(" min"));
                            }
                            routines::Step::Wait(s) => {
                                ui.label("Wait");
                                ui.add(egui::DragValue::new(s).range(1..=60).suffix(" s"));
                            }
                            other => {
                                ui.label(other.label());
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text("Remove this step").clicked() {
                                remove = Some(j);
                            }
                        });
                    });
                }
                if let Some(j) = remove {
                    r.steps.remove(j);
                }
                ui.menu_button("Add a step…", |ui| {
                    let mut add = None;
                    for (key, label) in routines::KEYS {
                        if ui.button(label).clicked() {
                            add = Some(routines::Step::Key(key));
                        }
                    }
                    ui.separator();
                    for (label, step) in [("Volume", routines::Step::Volume(20)), ("Sleep timer", routines::Step::Sleep(60)), ("Wait", routines::Step::Wait(5))] {
                        if ui.button(label).clicked() {
                            add = Some(step);
                        }
                    }
                    if !apps.is_empty() {
                        ui.menu_button("Open a channel", |ui| {
                            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                                for app in &apps {
                                    if ui.button(&app.name).clicked() {
                                        add = Some(routines::Step::Launch(app.id.clone(), app.name.clone()));
                                    }
                                }
                            });
                        });
                    }
                    if let Some(step) = add {
                        r.steps.push(step);
                        ui.close();
                    }
                });
            });
        }
        if let Some(i) = delete {
            self.routines.remove(i);
        }
        if ui.add(egui::Button::new("New routine").min_size(vec2(0.0, 32.0))).clicked() {
            self.routines.push(routines::Routine { name: "New routine".into(), steps: Vec::new() });
        }
        if self.routines != before {
            core::save_routines(&self.routines);
            refresh_tray();
        }
    }

    /// Shrink to a small strip that stays above other windows, or back.
    fn set_mini(&mut self, ctx: &egui::Context, mini: bool) {
        use egui::ViewportCommand as V;
        self.mini = mini;
        if mini {
            self.full_size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
            ctx.send_viewport_cmd(V::MinInnerSize(MINI_SIZE));
            ctx.send_viewport_cmd(V::InnerSize(MINI_SIZE));
            ctx.send_viewport_cmd(V::WindowLevel(egui::WindowLevel::AlwaysOnTop));
        } else {
            ctx.send_viewport_cmd(V::WindowLevel(egui::WindowLevel::Normal));
            ctx.send_viewport_cmd(V::InnerSize(self.full_size.unwrap_or(FULL_SIZE)));
            ctx.send_viewport_cmd(V::MinInnerSize(MIN_SIZE));
        }
    }

    /// The strip: what's on, and the keys used most.
    fn mini_ui(&mut self, ui: &mut egui::Ui) {
        let is_tv = self.device.as_ref().is_some_and(|d| d.is_tv);
        let line = match (&self.device, &self.now) {
            (Some(dev), Some(now)) => format!("{}  ·  {}", now.app, dev.name),
            (Some(dev), None) => dev.name.clone(),
            (None, _) => "Not connected".into(),
        };
        ui.vertical_centered(|ui| ui.add(egui::Label::new(egui::RichText::new(line).small().color(look::pal().weak)).truncate()));
        let (k, gap) = (vec2(48.0, 40.0), ui.spacing().item_spacing.x);
        let keys: &[(Icon, &'static str, &str)] = if is_tv {
            &[(Icon::Play, "Play", "Play / Pause"), (Icon::VolDown, "VolumeDown", "Volume down"), (Icon::Mute, "VolumeMute", "Mute"), (Icon::VolUp, "VolumeUp", "Volume up")]
        } else {
            &[(Icon::Rev, "Rev", "Rewind"), (Icon::Play, "Play", "Play / Pause"), (Icon::Fwd, "Fwd", "Fast forward")]
        };
        let n = keys.len() as f32 + 1.0;
        centered_row(ui, n * k.x + (n - 1.0) * gap, |ui| {
            ui.add_enabled_ui(self.device.is_some(), |ui| {
                for (icon, key, tip) in keys {
                    self.icon_key(ui, k, *icon, key, tip);
                }
            });
            if look::text_key(ui, k, "Full", false).on_hover_text("Back to the full remote").clicked() {
                self.set_mini(ui.ctx(), false);
            }
        });
    }

    fn channels(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("CHANNELS").small().strong().color(look::pal().weak));
            if !self.apps.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter…").margin(egui::Margin::symmetric(8, 4)).desired_width(110.0));
                });
            }
        });
        if self.apps.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Channel list unavailable. To show it, on the TV set Settings > System > \
                         Advanced system settings > Control by mobile apps to Enabled.",
                    )
                    .small()
                    .color(look::pal().weak),
                )
                .wrap(),
            );
            return;
        }
        let needle = self.filter.trim().to_lowercase();
        let mut shown: Vec<&ecp::App> = self.apps.iter().filter(|a| a.name.to_lowercase().contains(&needle)).collect();
        if shown.is_empty() {
            ui.label(egui::RichText::new("No channel has that in its name.").small().color(look::pal().weak));
            return;
        }
        // Pinned channels first; the sort is stable, so the rest keep the TV's order.
        shown.sort_by_key(|a| !self.favorites.contains(&a.id));
        let gap = 8.0;
        // Three across in a narrow window, more as it widens.
        let cols = (((ui.available_width() + gap) / (88.0 + gap)) as usize).clamp(3, 6);
        let tw = ((ui.available_width() - gap * (cols - 1) as f32) / cols as f32).floor();
        // Roku channel art is 290×218.
        let th = (tw * 218.0 / 290.0).round();
        let (mut launch, mut pin) = (None, None);
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        for row in shown.chunks(cols) {
            ui.horizontal(|ui| {
                for app in row {
                    let (rect, resp) = ui.allocate_exact_size(vec2(tw, th), egui::Sense::click());
                    let pinned = self.favorites.contains(&app.id);
                    let tip = format!("{}\nRight-click to {}", app.name, if pinned { "unpin" } else { "pin to the top" });
                    let resp = resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand);
                    let p = ui.painter();
                    let hover = if look::animated() { ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.15) } else { resp.hovered() as u8 as f32 };
                    let lift = -3.0 * hover;
                    let r = rect.translate(vec2(0.0, lift));
                    p.rect_filled(rect.translate(vec2(0.0, 3.0)), 10.0, egui::Color32::from_black_alpha(90));
                    p.rect_filled(r, 10.0, look::pal().key);
                    match self.icons.get(&app.id) {
                        Some(bytes) => {
                            egui::Image::from_bytes(format!("bytes://channel/{}.img", app.id), egui::load::Bytes::Shared(bytes.clone()))
                                .corner_radius(10)
                                .paint_at(ui, r);
                        }
                        None => {
                            let galley = ui.painter().layout(app.name.clone(), egui::FontId::proportional(13.0), look::pal().text, tw - 12.0);
                            ui.painter().galley(r.center() - galley.size() / 2.0, galley, look::pal().text);
                        }
                    }
                    let ring = if resp.is_pointer_button_down_on() {
                        Some(egui::Stroke::new(3.0, look::pal().accent))
                    } else if hover > 0.0 {
                        Some(egui::Stroke::new(2.0, look::pal().accent_soft.gamma_multiply(hover)))
                    } else {
                        None
                    };
                    if let Some(s) = ring {
                        ui.painter().rect_stroke(r, 10.0, s, egui::StrokeKind::Outside);
                    }
                    if pinned {
                        let at = r.right_top() + vec2(-12.0, 12.0);
                        ui.painter().circle_filled(at, 9.0, egui::Color32::from_black_alpha(170));
                        ui.painter().text(at, egui::Align2::CENTER_CENTER, "★", egui::FontId::proportional(12.0), look::WARN);
                    }
                    if resp.clicked() {
                        launch = Some(app.id.clone());
                    }
                    if resp.secondary_clicked() {
                        pin = Some(app.id.clone());
                    }
                }
            });
        }
        if let (Some(id), Some(dev)) = (launch, &self.device) {
            let _ = self.core.cmd.send(Cmd::Launch(dev.host.clone(), id));
        }
        if let Some(id) = pin {
            match self.favorites.iter().position(|f| *f == id) {
                Some(i) => drop(self.favorites.remove(i)),
                None => self.favorites.push(id),
            }
            core::save_favorites(&self.favorites);
        }
    }

    fn take_screenshot(&mut self, ui: &egui::Ui) {
        let Some((path, frames)) = &mut self.screenshot else { return };
        if *frames == u32::MAX {
            // Take the image out first: sending a viewport command while
            // holding the input lock deadlocks.
            let image = ui.input(|i| {
                i.raw.events.iter().find_map(|ev| match ev {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                let mut ppm = format!("P6 {} {} 255\n", image.size[0], image.size[1]).into_bytes();
                for px in &image.pixels {
                    ppm.extend_from_slice(&[px.r(), px.g(), px.b()]);
                }
                let _ = std::fs::write(&*path, ppm);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        *frames += 1;
        ui.ctx().request_repaint();
        // Wait for the channel logos, then a few more frames for them to decode.
        if (self.device.is_some() && self.now.is_some() && self.icons.len() >= self.apps.len().min(9) && *frames > 120) || *frames > 900 {
            *frames = u32::MAX;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}

const FULL_SIZE: Vec2 = vec2(380.0, 820.0);
const MIN_SIZE: Vec2 = vec2(340.0, 560.0);
const MINI_SIZE: Vec2 = vec2(300.0, 92.0);

/// Window widths where the layout gains a second and a third column.
const TWO_COLUMNS: f32 = 560.0;
const THREE_COLUMNS: f32 = 900.0;
const REMOTE_WIDTH: f32 = 340.0;
const TOOLS_WIDTH: f32 = 300.0;

/// A column `width` wide and `height` tall, laid out top to bottom.
fn column(ui: &mut egui::Ui, width: f32, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(vec2(width, height), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        add(ui);
    });
}

/// A vertical scroll area filling what's left, named so each column scrolls on its own.
fn scroll<R>(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) {
    let mut area = egui::ScrollArea::vertical().id_salt(id).auto_shrink(false);
    if id == "all"
        && let Some(y) = std::env::var("SOFA_REMOTE_SCROLL").ok().and_then(|y| y.parse().ok())
    {
        area = area.vertical_scroll_offset(y);
    }
    area.show(ui, add);
}

/// Animated keys on or off, the color theme, and the update check.
fn appearance(ui: &mut egui::Ui, updates: &mut bool) {
    let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
    ui.add_space(6.0);
    egui::Frame::new().fill(look::pal().body).corner_radius(16).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(egui::RichText::new("Appearance").strong());
        let mut animate = look::animated();
        let mut theme = look::theme();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label("Animated buttons");
                ui.label(weak("Keys fade, press in and ripple when clicked"));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                look::toggle(ui, &mut animate).on_hover_text(if animate { "On" } else { "Off" });
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Theme");
            ui.label(weak(look::THEMES[theme].0));
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, (name, palette)) in look::THEMES.iter().enumerate().take(look::MOVING) {
                if look::swatch(ui, palette, i == theme).on_hover_text(*name).clicked() {
                    theme = i;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Animated themes");
            ui.label(weak("The background moves"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, (name, palette)) in look::THEMES.iter().enumerate().skip(look::MOVING) {
                if look::swatch(ui, palette, i == theme).on_hover_text(*name).clicked() {
                    theme = i;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label("Check for updates");
                ui.label(weak("Asks GitHub at startup if a newer version is out"));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let was = *updates;
                look::toggle(ui, updates).on_hover_text(if was { "On" } else { "Off" });
                if *updates != was {
                    core::save_updates(*updates);
                }
            });
        });
        if animate != look::animated() || theme != look::theme() {
            look::set_animated(animate);
            look::set_theme(ui.ctx(), theme);
            core::save_appearance(look::THEMES[theme].0, animate);
        }
    });
}

/// Who made this and whose it isn't, with the open-source notices.
fn about(ui: &mut egui::Ui) {
    let version = env!("CARGO_PKG_VERSION");
    let weak = |t: &str| egui::RichText::new(t).small().color(look::pal().weak);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(weak(&format!("Sofa Remote {version} · unofficial")));
        ui.label(weak("Not affiliated with or endorsed by Roku, Inc."));
        ui.horizontal(|ui| {
            let links = [("Website", REPO.to_string()), ("Open-source licenses", format!("{REPO}/blob/v{version}/THIRD-PARTY-LICENSES.md"))];
            let w: f32 = links.iter().map(|(t, _)| t.len() as f32 * 6.2).sum::<f32>() + 24.0;
            ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
            for (text, url) in links {
                ui.hyperlink_to(egui::RichText::new(text).small(), url);
            }
        });
    });
    ui.add_space(6.0);
}

/// The window's view of the long-lived `Remote`, which survives the window
/// closing to the tray and reopening.
struct Window(Rc<RefCell<Remote>>);

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let mut remote = self.0.borrow_mut();
        remote.ui(ui, frame);
        if !ui.input(|i| i.pointer.any_down()) {
            remote.hold = None;
        }
    }
}

impl Remote {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll(&ui.ctx().clone());
        self.shortcuts(ui);
        self.take_screenshot(ui);
        if self.mini {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(look::pal().bg).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
                look::backdrop(ui, ui.max_rect().expand(10.0));
                self.mini_ui(ui);
            });
            return;
        }
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(look::pal().body).inner_margin(egui::Margin::symmetric(14, 8)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mini = look::text_key(ui, vec2(44.0, 22.0), "Mini", false);
                    if mini.on_hover_text("Shrink to a small strip with play and volume").clicked() {
                        self.set_mini(ui.ctx(), true);
                    }
                    if self.busy {
                        ui.spinner();
                    }
                    if self.status.is_empty() {
                        ui.label(egui::RichText::new("Arrows · Enter · Backspace · H · Space · +/− · M").small().color(look::pal().weak));
                    } else {
                        let color = if self.error { look::WARN } else { look::pal().text };
                        ui.add(egui::Label::new(egui::RichText::new(&self.status).small().color(color)).wrap());
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(look::pal().bg).inner_margin(egui::Margin::same(14)))
            .show(ui, |ui| {
                look::backdrop(ui, ui.max_rect().expand(14.0));
                self.header(ui);
                ui.add_space(4.0);
                let w = ui.available_width();
                if w < TWO_COLUMNS {
                    self.now_playing(ui);
                    scroll(ui, "all", |ui| {
                        ui.add_enabled_ui(self.device.is_some(), |ui| {
                            self.remote_card(ui);
                            ui.add_space(4.0);
                            self.tools(ui);
                            ui.add_space(4.0);
                            self.channels(ui);
                        });
                        ui.add_space(14.0);
                        about(ui);
                    });
                    return;
                }
                // Wider: the remote keeps its own column on the left, so it
                // never scrolls away under the rest.
                let gap = 14.0;
                let side = if w < THREE_COLUMNS { 0.0 } else { TOOLS_WIDTH };
                let middle = w - REMOTE_WIDTH - gap - if side > 0.0 { side + gap } else { 0.0 };
                let h = ui.available_height();
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    column(ui, REMOTE_WIDTH, h, |ui| {
                        scroll(ui, "remote", |ui| ui.add_enabled_ui(self.device.is_some(), |ui| self.remote_card(ui)));
                    });
                    column(ui, middle, h, |ui| {
                        self.now_playing(ui);
                        scroll(ui, "all", |ui| {
                            ui.add_enabled_ui(self.device.is_some(), |ui| {
                                if side == 0.0 {
                                    self.tools(ui);
                                    ui.add_space(4.0);
                                }
                                self.channels(ui);
                            });
                            ui.add_space(14.0);
                            about(ui);
                        });
                    });
                    if side > 0.0 {
                        column(ui, side, h, |ui| {
                            scroll(ui, "tools", |ui| ui.add_enabled_ui(self.device.is_some(), |ui| self.tools(ui)));
                        });
                    }
                });
            });
    }
}

fn main() -> eframe::Result {
    // Dev aid: open at another size, e.g. SOFA_REMOTE_SIZE=1000x700.
    let size = std::env::var("SOFA_REMOTE_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?])))
        .unwrap_or(FULL_SIZE.into());
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }
    let (core, events, wake) = Core::start();
    #[cfg(target_os = "linux")]
    let has_tray = {
        if let mpris::Start::AlreadyRunning = mpris::start(core.clone()) {
            return Ok(());
        }
        tray::start(core.clone())
    };
    #[cfg(not(target_os = "linux"))]
    let has_tray = false;

    let remote = Rc::new(RefCell::new(Remote::new(core.clone(), events)));
    // Started at login: stay in the tray until asked for.
    if has_tray && args.iter().any(|a| a == "--hidden") && !matches!(wake.recv(), Ok(Wake::Open)) {
        std::process::exit(0);
    }
    loop {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(size)
                .with_min_inner_size(if std::env::var_os("SOFA_REMOTE_MINI").is_some() { MINI_SIZE } else { MIN_SIZE })
                .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default())
                .with_app_id("io.github.billy1204.sofa-remote"),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        };
        let (window, state) = (remote.clone(), core.state.clone());
        eframe::run_native(
            "Sofa Remote",
            options,
            Box::new(move |cc| {
                let (theme, animate) = core::saved_appearance();
                look::set_animated(animate);
                look::set_theme(&cc.egui_ctx, look::THEMES.iter().position(|(n, _)| Some(*n) == theme.as_deref()).unwrap_or(0));
                egui_extras::install_image_loaders(&cc.egui_ctx);
                state.lock().unwrap().ctx = Some(cc.egui_ctx.clone());
                window.borrow_mut().reopened();
                refresh_tray();
                Ok(Box::new(Window(window)))
            }),
        )?;
        core.state.lock().unwrap().ctx = None;
        refresh_tray();
        // Closing the window hides it to the tray; the tray reopens it.
        if !has_tray || core.state.lock().unwrap().quit {
            break;
        }
        match wake.recv() {
            Ok(Wake::Open) => continue,
            Ok(Wake::Quit) | Err(_) => break,
        }
    }
    // The tray and network threads don't need to wind down.
    std::process::exit(0)
}
