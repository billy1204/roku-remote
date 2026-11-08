// No console window behind the remote on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod core;
mod ecp;
mod look;
#[cfg(target_os = "linux")]
mod mpris;
#[cfg(target_os = "linux")]
mod tray;

use eframe::egui::{self, Vec2, vec2};
use crate::core::{Cmd, Core, Event, Wake};
use look::Icon;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
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

const FLASH_SECS: f64 = 0.15;

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
    status: String,
    error: bool,
    busy: bool,
    /// Key last sent from the keyboard and when, so its button lights up.
    flash: Option<(&'static str, f64)>,
    /// Dev aid: save a screenshot here once the window has settled, then quit.
    screenshot: Option<(PathBuf, u32)>,
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
            status: String::new(),
            error: false,
            busy: false,
            flash: None,
            screenshot: std::env::var_os("ROKU_REMOTE_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
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

    /// The window was reopened from the tray: catch up on what happened
    /// while it was closed.
    fn reopened(&mut self) {
        self.title.clear();
        self.now = self.core.state.lock().unwrap().now.clone();
    }

    fn lit(&self, ui: &egui::Ui, key: &str) -> bool {
        self.flash.is_some_and(|(k, t)| k == key && ui.input(|i| i.time) - t < FLASH_SECS)
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let title = match (&self.device, &self.now) {
            (Some(dev), Some(now)) => format!("{} · {} — Roku Remote", now.app, dev.name),
            _ => "Roku Remote".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Found(devs) => {
                    if self.device.is_none() {
                        self.busy = false;
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
            let name = self.device.as_ref().map_or("Roku Remote".to_string(), |d| d.name.clone());
            let menu_w = ui.available_width() - 38.0 - ui.spacing().item_spacing.x;
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
        });
        ui.add_space(-4.0);
        ui.horizontal(|ui| {
            let (dot, sub) = match &self.device {
                Some(d) if d.limited => (look::WARN, format!("{}  ·  volume only", d.host)),
                Some(d) => (look::GOOD, format!("{}  ·  connected", d.host)),
                None if self.busy => (look::WEAK, "searching…".into()),
                None => (look::WEAK, "not connected".into()),
            };
            ui.add_space(4.0);
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
            ui.label(egui::RichText::new(sub).small().color(look::WEAK));
        });
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
    }

    /// One key: sends `key` on click, lights up while pressed or flashed.
    fn icon_key(&mut self, ui: &mut egui::Ui, size: Vec2, icon: Icon, key: &'static str, tip: &str) {
        let tint = (key == "PowerToggle").then_some(look::POWER);
        let lit = self.lit(ui, key);
        if look::icon_key(ui, size, icon, lit, tint).on_hover_text(tip).clicked() {
            self.key(key);
        }
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
        let mut pressed = None;
        ui.vertical_centered(|ui| pressed = look::dpad(ui, d, lit));
        if let Some(key) = pressed {
            self.key(key);
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
            let inputs = [("HDMI 1", "InputHDMI1"), ("HDMI 2", "InputHDMI2"), ("HDMI 3", "InputHDMI3"), ("Antenna", "InputTuner")];
            let iw = ((w - 3.0 * gap) / 4.0).min(74.0);
            centered_row(ui, 4.0 * iw + 3.0 * gap, |ui| {
                for (label, key) in inputs {
                    let lit = self.lit(ui, key);
                    if look::text_key(ui, vec2(iw, 30.0), label, lit).on_hover_text("Switch input").clicked() {
                        self.key(key);
                    }
                }
            });
        }
    }

    fn now_playing(&mut self, ui: &mut egui::Ui) {
        let Some(now) = self.now.clone() else { return };
        fn clock(ms: u64) -> String {
            let s = ms / 1000;
            if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
        }
        egui::Frame::new().fill(look::BODY).corner_radius(16).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (art, _) = ui.allocate_exact_size(vec2(64.0, 48.0), egui::Sense::hover());
                ui.painter().rect_filled(art, 8.0, look::KEY);
                match self.icons.get(&now.app_id) {
                    Some(bytes) => {
                        egui::Image::from_bytes(format!("bytes://channel/{}.img", now.app_id), egui::load::Bytes::Shared(bytes.clone()))
                            .corner_radius(8)
                            .paint_at(ui, art);
                    }
                    None => look::paint_icon(ui.painter(), art.shrink(12.0), Icon::Home, look::WEAK),
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
                    ui.label(egui::RichText::new(line).small().color(look::WEAK));
                    if now.duration_ms > 0 {
                        let (bar, _) = ui.allocate_exact_size(vec2(text_w, 4.0), egui::Sense::hover());
                        ui.painter().rect_filled(bar, 2.0, look::KEY);
                        let done = (now.position_ms as f32 / now.duration_ms as f32).clamp(0.0, 1.0);
                        let mut fill = bar;
                        fill.set_width(bar.width() * done);
                        ui.painter().rect_filled(fill, 2.0, look::ACCENT);
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
                    .hint_text("Type into a search box on the TV…")
                    .margin(egui::Margin::symmetric(12, 9))
                    .desired_width(ui.available_width() - 44.0 - ui.spacing().item_spacing.x),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let send = look::icon_key(ui, Vec2::splat(38.0), Icon::Send, false, Some(look::ACCENT)).on_hover_text("Send text");
            if (send.clicked() || enter)
                && !self.text.is_empty()
                && let Some(dev) = &self.device
            {
                let _ = self.core.cmd.send(Cmd::Text(dev.host.clone(), std::mem::take(&mut self.text)));
            }
        });
    }

    fn channels(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("CHANNELS").small().strong().color(look::WEAK));
        if self.apps.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Channel list unavailable. To show it, on the TV set Settings > System > \
                         Advanced system settings > Control by mobile apps to Enabled.",
                    )
                    .small()
                    .color(look::WEAK),
                )
                .wrap(),
            );
            return;
        }
        let cols = 3;
        let gap = 8.0;
        let tw = ((ui.available_width() - gap * (cols - 1) as f32) / cols as f32).floor();
        // Roku channel art is 290×218.
        let th = (tw * 218.0 / 290.0).round();
        let mut launch = None;
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        for row in self.apps.chunks(cols) {
            ui.horizontal(|ui| {
                for app in row {
                    let (rect, resp) = ui.allocate_exact_size(vec2(tw, th), egui::Sense::click());
                    let resp = resp.on_hover_text(&app.name).on_hover_cursor(egui::CursorIcon::PointingHand);
                    let p = ui.painter();
                    let lift = if resp.hovered() { -2.0 } else { 0.0 };
                    let r = rect.translate(vec2(0.0, lift));
                    p.rect_filled(rect.translate(vec2(0.0, 3.0)), 10.0, egui::Color32::from_black_alpha(90));
                    p.rect_filled(r, 10.0, look::KEY);
                    match self.icons.get(&app.id) {
                        Some(bytes) => {
                            egui::Image::from_bytes(format!("bytes://channel/{}.img", app.id), egui::load::Bytes::Shared(bytes.clone()))
                                .corner_radius(10)
                                .paint_at(ui, r);
                        }
                        None => {
                            let galley = ui.painter().layout(app.name.clone(), egui::FontId::proportional(13.0), look::TEXT, tw - 12.0);
                            ui.painter().galley(r.center() - galley.size() / 2.0, galley, look::TEXT);
                        }
                    }
                    let ring = if resp.is_pointer_button_down_on() {
                        Some(egui::Stroke::new(3.0, look::ACCENT))
                    } else if resp.hovered() {
                        Some(egui::Stroke::new(2.0, look::ACCENT_SOFT))
                    } else {
                        None
                    };
                    if let Some(s) = ring {
                        ui.painter().rect_stroke(r, 10.0, s, egui::StrokeKind::Outside);
                    }
                    if resp.clicked() {
                        launch = Some(app.id.clone());
                    }
                }
            });
        }
        if let (Some(id), Some(dev)) = (launch, &self.device) {
            let _ = self.core.cmd.send(Cmd::Launch(dev.host.clone(), id));
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

/// The window's view of the long-lived `Remote`, which survives the window
/// closing to the tray and reopening.
struct Window(Rc<RefCell<Remote>>);

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.0.borrow_mut().ui(ui, frame);
    }
}

impl Remote {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll(&ui.ctx().clone());
        self.shortcuts(ui);
        self.take_screenshot(ui);
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(look::BODY).inner_margin(egui::Margin::symmetric(14, 8)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    if self.status.is_empty() {
                        ui.label(egui::RichText::new("Arrows · Enter · Backspace · H · Space · +/− · M").small().color(look::WEAK));
                    } else {
                        let color = if self.error { look::WARN } else { look::TEXT };
                        ui.add(egui::Label::new(egui::RichText::new(&self.status).small().color(color)).wrap());
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(look::BG).inner_margin(egui::Margin::same(14)))
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(4.0);
                self.now_playing(ui);
                let mut scroll = egui::ScrollArea::vertical().auto_shrink(false);
                if let Some(y) = std::env::var("ROKU_REMOTE_SCROLL").ok().and_then(|y| y.parse().ok()) {
                    scroll = scroll.vertical_scroll_offset(y);
                }
                scroll.show(ui, |ui| {
                    ui.add_enabled_ui(self.device.is_some(), |ui| {
                        egui::Frame::new()
                            .fill(look::BODY)
                            .corner_radius(28)
                            .inner_margin(egui::Margin::symmetric(14, 20))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.remote(ui);
                            });
                        ui.add_space(4.0);
                        self.search(ui);
                        ui.add_space(4.0);
                        self.channels(ui);
                    });
                });
            });
    }
}

fn main() -> eframe::Result {
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
    loop {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([380.0, 820.0])
                .with_min_inner_size([340.0, 560.0])
                .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default())
                .with_app_id("roku-remote"),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        };
        let (window, state) = (remote.clone(), core.state.clone());
        eframe::run_native(
            "Roku Remote",
            options,
            Box::new(move |cc| {
                look::install(&cc.egui_ctx);
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
