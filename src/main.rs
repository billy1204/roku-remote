// No console window behind the remote on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod ecp;

use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

enum Cmd {
    Discover,
    Connect(String),
    Key(String, &'static str),
    Text(String, String),
    Launch(String, String),
}

enum Event {
    Found(Vec<ecp::Device>),
    Connected(ecp::Device, Vec<ecp::App>),
    Error(String),
    Ok,
}

/// Network calls run on one worker thread so the window never blocks, and
/// keypresses reach the TV in the order they were pressed.
fn spawn_worker(ctx: egui::Context) -> (Sender<Cmd>, Receiver<Event>) {
    let (cmd_tx, cmd_rx) = channel::<Cmd>();
    let (ev_tx, ev_rx) = channel();
    std::thread::spawn(move || {
        for cmd in cmd_rx {
            let ev = match cmd {
                Cmd::Discover => Event::Found(ecp::discover()),
                Cmd::Connect(host) => match ecp::device_info(&host) {
                    Ok(dev) => Event::Connected(dev, ecp::apps(&host).unwrap_or_default()),
                    Err(e) => Event::Error(e),
                },
                Cmd::Key(host, key) => ecp::keypress(&host, key).map_or_else(Event::Error, |_| Event::Ok),
                Cmd::Text(host, text) => ecp::type_text(&host, &text).map_or_else(Event::Error, |_| Event::Ok),
                Cmd::Launch(host, id) => ecp::launch(&host, &id).map_or_else(Event::Error, |_| Event::Ok),
            };
            if ev_tx.send(ev).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    (cmd_tx, ev_rx)
}

fn config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("APPDATA")?);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("roku-remote").join("host"))
}

fn save_host(host: &str) {
    if let Some(path) = config_path() {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, host);
    }
}

struct Remote {
    tx: Sender<Cmd>,
    rx: Receiver<Event>,
    host_input: String,
    device: Option<ecp::Device>,
    found: Vec<ecp::Device>,
    apps: Vec<ecp::App>,
    text: String,
    status: String,
    busy: bool,
}

impl Remote {
    fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = spawn_worker(ctx);
        let saved = config_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string());
        let mut app = Self {
            tx,
            rx,
            host_input: saved.clone().unwrap_or_default(),
            device: None,
            found: Vec::new(),
            apps: Vec::new(),
            text: String::new(),
            status: String::new(),
            busy: false,
        };
        match saved.filter(|s| !s.is_empty()) {
            Some(host) => app.send(Cmd::Connect(host), "Connecting…"),
            None => app.send(Cmd::Discover, "Searching for Roku devices…"),
        }
        app
    }

    fn send(&mut self, cmd: Cmd, status: &str) {
        self.busy = matches!(cmd, Cmd::Discover | Cmd::Connect(_));
        self.status = status.to_string();
        let _ = self.tx.send(cmd);
    }

    fn key(&mut self, key: &'static str) {
        if let Some(dev) = &self.device {
            let _ = self.tx.send(Cmd::Key(dev.host.clone(), key));
        }
    }

    fn poll(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Found(devs) => {
                    self.busy = false;
                    self.status = match devs.len() {
                        0 => "No Roku found. Enter its IP address (Settings > Network > About).".into(),
                        1 => String::new(),
                        n => format!("Found {n} devices."),
                    };
                    if devs.len() == 1 && self.device.is_none() {
                        self.host_input = devs[0].host.clone();
                        let host = self.host_input.clone();
                        self.send(Cmd::Connect(host), "Connecting…");
                    }
                    self.found = devs;
                }
                Event::Connected(dev, apps) => {
                    self.busy = false;
                    save_host(&dev.host);
                    self.status = if dev.limited {
                        "This TV only accepts volume from the network. On the TV, set Settings > \
                         System > Advanced system settings > Control by mobile apps to Enabled."
                            .into()
                    } else {
                        String::new()
                    };
                    self.host_input = dev.host.clone();
                    self.device = Some(dev);
                    self.apps = apps;
                }
                Event::Error(e) => {
                    self.busy = false;
                    self.status = e;
                }
                Event::Ok => {
                    if !self.busy {
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
        let pressed: Vec<&'static str> =
            ui.input(|i| map.iter().filter(|(k, _)| i.key_pressed(*k)).map(|(_, v)| *v).collect());
        for key in pressed {
            self.key(key);
        }
    }

    fn device_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = self.device.as_ref().map_or("Not connected".to_string(), |d| d.name.clone());
            egui::ComboBox::from_id_salt("device")
                .selected_text(label)
                .width(ui.available_width() - 40.0)
                .show_ui(ui, |ui| {
                    let mut pick = None;
                    for d in &self.found {
                        let current = self.device.as_ref().is_some_and(|c| c.host == d.host);
                        if ui.selectable_label(current, format!("{}  ({})", d.name, d.host)).clicked() {
                            pick = Some(d.host.clone());
                        }
                    }
                    if self.found.is_empty() {
                        ui.weak("No devices found yet");
                    }
                    if let Some(host) = pick {
                        self.send(Cmd::Connect(host), "Connecting…");
                    }
                });
            if ui.add_enabled(!self.busy, egui::Button::new("⟳")).on_hover_text("Search the network").clicked() {
                self.send(Cmd::Discover, "Searching for Roku devices…");
            }
        });
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.host_input)
                    .hint_text("IP address, e.g. 192.168.1.50")
                    .desired_width(ui.available_width() - 80.0),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.add_enabled(!self.busy, egui::Button::new("Connect")).clicked() || enter)
                && !self.host_input.trim().is_empty()
            {
                let host = self.host_input.trim().to_string();
                self.send(Cmd::Connect(host), "Connecting…");
            }
        });
    }

    fn remote(&mut self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let gap = ui.spacing().item_spacing.x;
        let third = (w - 2.0 * gap) / 3.0;
        let btn = |ui: &mut egui::Ui, text: &str, width: f32, height: f32| {
            ui.add_sized([width, height], egui::Button::new(egui::RichText::new(text).size(18.0)))
        };
        let mut pressed: Option<&'static str> = None;
        let row = |ui: &mut egui::Ui, keys: &[(&str, &'static str)], height: f32, pressed: &mut Option<&'static str>| {
            let width = (w - gap * (keys.len() as f32 - 1.0)) / keys.len() as f32;
            ui.horizontal(|ui| {
                for (label, key) in keys {
                    if key.is_empty() {
                        ui.add_space(width + gap);
                        continue;
                    }
                    if btn(ui, label, width, height).clicked() {
                        *pressed = Some(key);
                    }
                }
            });
        };

        let is_tv = self.device.as_ref().is_some_and(|d| d.is_tv);
        if is_tv {
            row(ui, &[("⏻ Power", "PowerToggle"), ("⌂ Home", "Home")], 36.0, &mut pressed);
        } else {
            row(ui, &[("⌂ Home", "Home")], 36.0, &mut pressed);
        }
        row(ui, &[("⮌ Back", "Back"), ("", ""), ("✱ Options", "Info")], 36.0, &mut pressed);
        ui.add_space(6.0);
        row(ui, &[("", ""), ("▲", "Up"), ("", "")], 52.0, &mut pressed);
        ui.horizontal(|ui| {
            for (label, key) in [("◀", "Left"), ("OK", "Select"), ("▶", "Right")] {
                if btn(ui, label, third, 52.0).clicked() {
                    pressed = Some(key);
                }
            }
        });
        row(ui, &[("", ""), ("▼", "Down"), ("", "")], 52.0, &mut pressed);
        ui.add_space(6.0);
        row(ui, &[("↺", "InstantReplay"), ("⏪", "Rev"), ("⏯", "Play"), ("⏩", "Fwd")], 40.0, &mut pressed);
        if is_tv {
            row(ui, &[("🔉", "VolumeDown"), ("🔇", "VolumeMute"), ("🔊", "VolumeUp")], 40.0, &mut pressed);
            row(ui, &[("HDMI1", "InputHDMI1"), ("HDMI2", "InputHDMI2"), ("HDMI3", "InputHDMI3"), ("Antenna", "InputTuner")], 30.0, &mut pressed);
        }
        if let Some(key) = pressed {
            self.key(key);
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .hint_text("Type into a search box…")
                    .desired_width(w - 60.0),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Send").clicked() || enter)
                && !self.text.is_empty()
                && let Some(dev) = &self.device
            {
                let _ = self.tx.send(Cmd::Text(dev.host.clone(), std::mem::take(&mut self.text)));
            }
        });

        if self.apps.is_empty() {
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Channel list unavailable. To show it, on the TV set Settings > System > \
                         Advanced system settings > Control by mobile apps to Enabled.",
                    )
                    .weak()
                    .small(),
                )
                .wrap(),
            );
        } else {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Channels").strong());
            let mut launch = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                let cols = 2;
                let width = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
                egui::Grid::new("apps").num_columns(cols).spacing([gap, gap]).show(ui, |ui| {
                    for (i, app) in self.apps.iter().enumerate() {
                        if ui.add_sized([width, 28.0], egui::Button::new(&app.name).truncate()).clicked() {
                            launch = Some(app.id.clone());
                        }
                        if i % cols == cols - 1 {
                            ui.end_row();
                        }
                    }
                });
            });
            if let (Some(id), Some(dev)) = (launch, &self.device) {
                let _ = self.tx.send(Cmd::Launch(dev.host.clone(), id));
            }
        }
    }
}

impl eframe::App for Remote {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        self.shortcuts(ui);
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.busy {
                    ui.spinner();
                }
                if self.status.is_empty() {
                    ui.weak("Keys: arrows, Enter, Backspace, H, Space, +/−, M");
                } else {
                    ui.add(egui::Label::new(&self.status).wrap());
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            self.device_bar(ui);
            ui.separator();
            ui.add_enabled_ui(self.device.is_some(), |ui| self.remote(ui));
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([360.0, 720.0])
            .with_min_inner_size([320.0, 520.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default())
            .with_app_id("roku-remote"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Roku Remote",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            Ok(Box::new(Remote::new(cc.egui_ctx.clone())))
        }),
    )
}
