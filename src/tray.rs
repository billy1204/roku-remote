//! System tray icon (StatusNotifierItem): volume, mute, power and play/pause
//! without opening the window. Scrolling on the icon changes the volume.
use crate::core::{Cmd, Core};
use crate::ecp::State;
use ksni::menu::{RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{Icon, MenuItem, Orientation, ToolTip};
use std::sync::OnceLock;

static HANDLE: OnceLock<ksni::blocking::Handle<Tray>> = OnceLock::new();

pub struct Tray {
    core: Core,
}

/// Show the tray icon. False when the desktop has no tray to show it in.
pub fn start(core: Core) -> bool {
    use ksni::blocking::TrayMethods;
    match (Tray { core }).spawn() {
        Ok(handle) => HANDLE.set(handle).is_ok(),
        Err(e) => {
            eprintln!("roku-remote: no system tray available ({e}); closing the window quits");
            false
        }
    }
}

/// Redraw the icon's tooltip and menu from the shared state.
pub fn refresh() {
    if let Some(handle) = HANDLE.get() {
        handle.update(|_| {});
    }
}

fn app_icon() -> Vec<Icon> {
    let Ok(img) = image::load_from_memory(include_bytes!("../assets/icon.png")) else { return Vec::new() };
    [32u32, 64, 256]
        .into_iter()
        .map(|size| {
            let rgba = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).into_rgba8();
            // RGBA -> ARGB (network byte order) as StatusNotifierItem expects.
            let data = rgba.pixels().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
            Icon { width: size as i32, height: size as i32, data }
        })
        .collect()
}

fn item(label: &str, icon: &str, enabled: bool, activate: impl Fn(&mut Tray) + Send + 'static) -> MenuItem<Tray> {
    StandardItem { label: label.into(), icon_name: icon.into(), enabled, activate: Box::new(activate), ..Default::default() }.into()
}

fn key_item(label: &str, icon: &str, enabled: bool, key: &'static str) -> MenuItem<Tray> {
    item(label, icon, enabled, move |t| t.core.key(key))
}

impl Tray {
    /// "HBO Max · Playing", or why there's nothing to show.
    fn status(&self) -> String {
        let st = self.core.state.lock().unwrap();
        match (&st.device, &st.now) {
            (None, _) => "Not connected".into(),
            (Some(_), None) => "Connected".into(),
            (Some(_), Some(now)) => match now.state {
                State::Playing => format!("{} · Playing", now.app),
                State::Paused => format!("{} · Paused", now.app),
                State::Stopped => now.app.clone(),
            },
        }
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "roku-remote".into()
    }
    fn title(&self) -> String {
        "Roku Remote".into()
    }
    fn icon_pixmap(&self) -> Vec<Icon> {
        static ICON: OnceLock<Vec<Icon>> = OnceLock::new();
        ICON.get_or_init(app_icon).clone()
    }
    fn tool_tip(&self) -> ToolTip {
        let name = self.core.state.lock().unwrap().device.as_ref().map(|d| d.name.clone());
        ToolTip { title: name.unwrap_or_else(|| "Roku Remote".into()), description: self.status(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.core.toggle_window();
    }
    fn scroll(&mut self, delta: i32, orientation: Orientation) {
        let is_tv = self.core.state.lock().unwrap().device.as_ref().is_some_and(|d| d.is_tv);
        if is_tv && matches!(orientation, Orientation::Vertical) && delta != 0 {
            self.core.key(if delta > 0 { "VolumeUp" } else { "VolumeDown" });
        }
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let (device, found, state) = {
            let st = self.core.state.lock().unwrap();
            (st.device.clone(), st.found.clone(), st.now.as_ref().map(|n| n.state))
        };
        let connected = device.is_some();
        let is_tv = device.as_ref().is_some_and(|d| d.is_tv);
        let mut items = vec![
            StandardItem { label: self.status(), enabled: false, ..Default::default() }.into(),
            MenuItem::Separator,
        ];
        if matches!(state, Some(State::Playing | State::Paused)) {
            let (label, icon) =
                if state == Some(State::Playing) { ("Pause", "media-playback-pause") } else { ("Play", "media-playback-start") };
            items.push(key_item(label, icon, true, "Play"));
        }
        if is_tv || !connected {
            items.extend([
                key_item("Volume Up", "audio-volume-high", connected, "VolumeUp"),
                key_item("Volume Down", "audio-volume-low", connected, "VolumeDown"),
                key_item("Mute", "audio-volume-muted", connected, "VolumeMute"),
                MenuItem::Separator,
                key_item("Power", "system-shutdown", connected, "PowerToggle"),
            ]);
        }
        if !found.is_empty() {
            let selected = device.as_ref().and_then(|d| found.iter().position(|f| f.host == d.host)).unwrap_or(usize::MAX);
            let hosts: Vec<String> = found.iter().map(|d| d.host.clone()).collect();
            items.push(
                SubMenu {
                    label: "TV".into(),
                    icon_name: "video-display".into(),
                    submenu: vec![
                        RadioGroup {
                            selected,
                            select: Box::new(move |t: &mut Self, i| {
                                if let Some(host) = hosts.get(i) {
                                    let _ = t.core.cmd.send(Cmd::Connect(host.clone()));
                                }
                            }),
                            options: found.iter().map(|d| RadioItem { label: d.name.clone(), ..Default::default() }).collect(),
                        }
                        .into(),
                    ],
                    ..Default::default()
                }
                .into(),
            );
        }
        let sharing = self.core.state.lock().unwrap().share.as_ref().map(|s| s.folder().to_path_buf());
        let folder_name = |p: &std::path::Path| p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
        match sharing {
            Some(folder) => items.push(item(&format!("Stop Sharing “{}”", folder_name(&folder)), "folder-remote", true, |t| t.core.stop_share())),
            None => {
                if let Some(last) = crate::core::saved_share() {
                    let label = format!("Share “{}”", folder_name(&last));
                    items.push(item(&label, "folder-remote", true, move |t| {
                        if let Err(e) = t.core.start_share(&last) {
                            eprintln!("roku-remote: couldn't share {}: {e}", last.display());
                        }
                    }));
                }
            }
        }
        items.extend([
            MenuItem::Separator,
            if self.core.window_open() {
                item("Hide Remote", "window-close", true, |t| t.core.toggle_window())
            } else {
                item("Show Remote", "window-new", true, |t| t.core.show_window())
            },
            item("Quit", "application-exit", true, |t| t.core.quit()),
        ]);
        items
    }
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // Keep the item; it reappears when the panel comes back.
        true
    }
}
