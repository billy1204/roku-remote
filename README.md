# Roku Remote

A small desktop remote for Roku TVs and streaming players on your home network, for Linux (AppImage) and Windows (.exe). Written in Rust with [egui](https://github.com/emilk/egui).

<img src="assets/icon.svg" width="64" alt="Roku Remote icon">

## Features

- **Finds your Rokus automatically** (SSDP), or connect by IP address. Remembers the last device.
- **Full remote:** Power, Home, Back, Options, D-pad and OK, Replay, Rewind, Play/Pause, Fast-forward.
- **TV controls** (Roku TVs only): Volume up/down/mute and HDMI 1–3/Antenna inputs.
- **Type into search boxes** from your keyboard instead of the on-screen keyboard.
- **Channel launcher:** a button for every installed channel.
- **Now playing:** the current channel, play state and progress, in the window and the title bar.
- **Linux desktop integration:**
  - **Tray icon:** play/pause, volume, mute, power and switching TVs without opening the window. Scroll on the icon to change the volume; click it to show or hide the remote. Closing the window keeps the remote in the tray.
  - **Media controls:** the TV appears in your desktop's media controls (COSMIC, GNOME, KDE) with the channel's logo, and your keyboard's media keys control it.
- **Keyboard shortcuts:**

  | Key | Action |
  |---|---|
  | Arrow keys | D-pad |
  | Enter | OK |
  | Backspace / Esc | Back |
  | H | Home |
  | Space | Play/Pause |
  | `,` / `.` | Rewind / Fast-forward |
  | R | Instant replay |
  | I | Options |
  | `+` / `-` | Volume up / down |
  | M | Mute |

## Setting up your Roku

Newer Roku firmware only accepts volume commands from the network by default. On each TV, go to:

**Settings > System > Advanced system settings > Control by mobile apps > Network access** and choose **Enabled**.

If a TV is still on Limited, the app tells you when you connect.

## Run it

Download from the [latest release](https://github.com/billy1204/roku-remote/releases/latest), or build it yourself (below).

**Linux:** download `Roku-Remote-x86_64.AppImage`, then:

```bash
chmod +x Roku-Remote-x86_64.AppImage
./Roku-Remote-x86_64.AppImage
```

**Windows:** download `Roku-Remote-windows-x86_64.exe` and double-click it. It's not code-signed, so Windows SmartScreen may warn you the first time; choose **More info > Run anyway**. If Windows Firewall asks, allow it on private networks so it can find your Rokus.

Your computer and the Roku must be on the same network.

## Build

Requires a Rust toolchain (edition 2024) and [appimagetool](https://github.com/AppImage/appimagetool).

```bash
cargo run --release                       # run directly
APPIMAGETOOL=/path/to/appimagetool packaging/build_appimage.sh   # build the AppImage
```

On Windows, `cargo build --release` produces `target\release\roku-remote.exe`. Publishing a GitHub release also builds the `.exe` and attaches it automatically ([workflow](.github/workflows/windows.yml)).

## How it works

It uses Roku's [External Control Protocol](https://developer.roku.com/docs/developer-program/dev-tools/external-control-api.md): plain HTTP requests to port 8060 on the device. All of that lives in [`src/ecp.rs`](src/ecp.rs). The UI and a background network thread are in [`src/main.rs`](src/main.rs).

## License

[MIT](LICENSE)
