//! Windows: the tray icon, the system media controls (media keys and the
//! volume overlay) and noticing a copy that's already running. The same
//! jobs `tray` and `mpris` do on Linux.
use crate::core::{Cmd, Core};
use crate::ecp::State;
use crate::routines::Step;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage};

/// Set when the shared state changes; the tray thread redraws from it.
static DIRTY: AtomicBool = AtomicBool::new(true);

/// Redraw the tray's menu and tooltip and the media controls.
pub fn refresh() {
    DIRTY.store(true, Ordering::Relaxed);
}

/// A handle another launch signals to bring this copy's window up.
pub struct ShowRequest(isize);

/// `None` when a copy is already running; it has been asked to show its
/// window.
pub fn single_instance() -> Option<ShowRequest> {
    let name: Vec<u16> = "Local\\SofaRemoteShow".encode_utf16().chain([0]).collect();
    // SAFETY: plain Win32 calls with a NUL-terminated name that outlives them.
    unsafe {
        let event = CreateEventW(std::ptr::null(), 0, 0, name.as_ptr());
        if !event.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
            SetEvent(event);
            return None;
        }
        Some(ShowRequest(event as isize))
    }
}

/// What a menu item does.
enum Action {
    Key(&'static str),
    Connect(String),
    Sleep(Option<u32>),
    Routine(Vec<Step>),
    Share(PathBuf),
    StopShare,
    Toggle,
    Quit,
}

/// Show the tray icon, on a thread of its own that also answers its
/// clicks. False when it couldn't be shown; closing the window then quits.
pub fn start(core: Core, show: ShowRequest) -> bool {
    let (ready, started) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Some(tray) = icon().and_then(|icon| {
            TrayIconBuilder::new().with_tooltip("Sofa Remote").with_icon(icon).with_menu_on_left_click(false).build().ok()
        }) else {
            let _ = ready.send(false);
            return;
        };
        let _ = ready.send(true);
        run(core, tray, show);
    });
    started.recv().unwrap_or(false)
}

fn icon() -> Option<Icon> {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).ok()?;
    let rgba = img.resize_exact(64, 64, image::imageops::FilterType::Lanczos3).into_rgba8();
    Icon::from_rgba(rgba.into_raw(), 64, 64).ok()
}

/// The tray thread: pump its window's messages, and act on clicks, menu
/// choices, media keys, another launch asking for the window, and changes
/// to the shared state.
fn run(core: Core, tray: TrayIcon, show: ShowRequest) {
    let mut media = media_controls(&core, &tray);
    let mut actions: Vec<(MenuId, Action)> = Vec::new();
    let event = show.0 as *mut std::ffi::c_void;
    let handles = [event];
    let count = if event.is_null() { 0 } else { 1 };
    loop {
        // SAFETY: `handles` and `msg` outlive the calls; the messages are this thread's own.
        unsafe {
            // Twice a second at least, to pick up changes to the state.
            if MsgWaitForMultipleObjects(count, handles.as_ptr(), 0, 500, QS_ALLINPUT) == WAIT_OBJECT_0 && count == 1 {
                core.show_window();
            }
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                core.toggle_window();
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            match actions.iter().find(|(id, _)| *id == event.id).map(|(_, a)| a) {
                Some(Action::Key(key)) => core.key(key),
                Some(Action::Connect(host)) => drop(core.cmd.send(Cmd::Connect(host.clone()))),
                Some(Action::Sleep(minutes)) => core.set_sleep(*minutes),
                Some(Action::Routine(steps)) => core.routine(steps.clone()),
                Some(Action::Share(folder)) => drop(core.start_share(folder)),
                Some(Action::StopShare) => core.stop_share(),
                Some(Action::Toggle) => core.toggle_window(),
                Some(Action::Quit) => core.quit(),
                None => {}
            }
        }
        if DIRTY.swap(false, Ordering::Relaxed) {
            let (menu, new_actions) = menu(&core);
            actions = new_actions;
            tray.set_menu(Some(Box::new(menu)));
            let _ = tray.set_tooltip(Some(core.status()));
            if let Some(media) = &mut media {
                update_media(&core, media);
            }
        }
    }
}

/// The menu, and what each of its items does.
fn menu(core: &Core) -> (Menu, Vec<(MenuId, Action)>) {
    let (device, found, state, sharing) = {
        let st = core.state.lock().unwrap();
        (st.device.clone(), st.found.clone(), st.now.as_ref().map(|n| n.state), st.share.as_ref().map(|s| s.folder().to_path_buf()))
    };
    let menu = Menu::new();
    let mut actions = Vec::new();
    let mut add = |to: &dyn Fn(&MenuItem), label: &str, action: Action| {
        let item = MenuItem::new(label, true, None);
        to(&item);
        actions.push((item.id().clone(), action));
    };
    let top = |item: &MenuItem| drop(menu.append(item));
    let separator = || drop(menu.append(&PredefinedMenuItem::separator()));
    let _ = menu.append(&MenuItem::new(core.status(), false, None));
    separator();
    let connected = device.is_some();
    let is_tv = device.as_ref().is_some_and(|d| d.is_tv);
    if matches!(state, Some(State::Playing | State::Paused)) {
        add(&top, if state == Some(State::Playing) { "Pause" } else { "Play" }, Action::Key("Play"));
    }
    if is_tv {
        add(&top, "Volume Up", Action::Key("VolumeUp"));
        add(&top, "Volume Down", Action::Key("VolumeDown"));
        add(&top, "Mute", Action::Key("VolumeMute"));
        separator();
        add(&top, "Power", Action::Key("PowerToggle"));
        let left = core.sleep_left();
        let sleep = Submenu::new(left.map_or("Sleep Timer".to_string(), |left| format!("Sleep Timer ({left} min left)")), true);
        let into = |item: &MenuItem| drop(sleep.append(item));
        for minutes in [30, 60, 90] {
            add(&into, &format!("{minutes} minutes"), Action::Sleep(Some(minutes)));
        }
        if left.is_some() {
            add(&into, "Cancel", Action::Sleep(None));
        }
        let _ = menu.append(&sleep);
    }
    let routines = crate::core::saved_routines();
    if connected && !routines.is_empty() {
        let sub = Submenu::new("Routines", true);
        let into = |item: &MenuItem| drop(sub.append(item));
        for r in routines {
            add(&into, &r.name, Action::Routine(r.steps));
        }
        let _ = menu.append(&sub);
    }
    if !found.is_empty() {
        let sub = Submenu::new("TV", true);
        let into = |item: &MenuItem| drop(sub.append(item));
        for d in &found {
            let current = device.as_ref().is_some_and(|c| c.host == d.host);
            add(&into, &format!("{}{}", if current { "\u{2022} " } else { "" }, d.name), Action::Connect(d.host.clone()));
        }
        let _ = menu.append(&sub);
    }
    let folder_name = |p: &std::path::Path| p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
    match sharing {
        Some(folder) => add(&top, &format!("Stop Sharing \u{201c}{}\u{201d}", folder_name(&folder)), Action::StopShare),
        None => {
            if let Some(last) = crate::core::saved_share() {
                add(&top, &format!("Share \u{201c}{}\u{201d}", folder_name(&last)), Action::Share(last));
            }
        }
    }
    separator();
    add(&top, if core.window_open() { "Hide Remote" } else { "Show Remote" }, Action::Toggle);
    add(&top, "Quit", Action::Quit);
    (menu, actions)
}

/// Windows' media controls, attached to the tray's own hidden window.
fn media_controls(core: &Core, tray: &TrayIcon) -> Option<MediaControls> {
    let config = PlatformConfig { display_name: "Sofa Remote", dbus_name: "sofa_remote", hwnd: Some(tray.window_handle() as *mut std::ffi::c_void) };
    let mut controls = MediaControls::new(config).ok()?;
    let core = core.clone();
    controls
        .attach(move |event| {
            let playing = core.state.lock().unwrap().now.as_ref().is_some_and(|n| n.state == State::Playing);
            match event {
                MediaControlEvent::Toggle => core.key("Play"),
                MediaControlEvent::Play if !playing => core.key("Play"),
                MediaControlEvent::Pause if playing => core.key("Play"),
                MediaControlEvent::Next => core.key("Fwd"),
                MediaControlEvent::Previous => core.key("InstantReplay"),
                MediaControlEvent::Stop => core.key("Back"),
                _ => {}
            }
        })
        .ok()?;
    Some(controls)
}

fn update_media(core: &Core, media: &mut MediaControls) {
    let (device, now) = {
        let st = core.state.lock().unwrap();
        (st.device.as_ref().map(|d| d.name.clone()), st.now.clone())
    };
    let playback = match now.as_ref().map(|n| n.state) {
        Some(State::Playing) => MediaPlayback::Playing { progress: None },
        Some(State::Paused) => MediaPlayback::Paused { progress: None },
        _ => MediaPlayback::Stopped,
    };
    let _ = media.set_metadata(MediaMetadata { title: now.as_ref().map(|n| n.app.as_str()), artist: device.as_deref(), ..Default::default() });
    let _ = media.set_playback(playback);
}
