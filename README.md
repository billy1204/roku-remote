# Roku Remote

A small desktop remote for Roku TVs and streaming players on your home network, packaged as a Linux AppImage. Written in Rust with [egui](https://github.com/emilk/egui).

<img src="assets/icon.svg" width="64" alt="Roku Remote icon">

## Features

- **Finds your Rokus automatically** (SSDP), or connect by IP address. Remembers the last device.
- **Full remote:** Power, Home, Back, Options, D-pad and OK, Replay, Rewind, Play/Pause, Fast-forward.
- **TV controls** (Roku TVs only): Volume up/down/mute and HDMI 1–3/Antenna inputs.
- **Type into search boxes** from your keyboard instead of the on-screen keyboard.
- **Channel launcher:** a button for every installed channel.
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

Download `Roku-Remote-x86_64.AppImage` (or build it, below), then:

```bash
chmod +x Roku-Remote-x86_64.AppImage
./Roku-Remote-x86_64.AppImage
```

Your computer and the Roku must be on the same network.

## Build

Requires a Rust toolchain (edition 2024) and [appimagetool](https://github.com/AppImage/appimagetool).

```bash
cargo run --release                       # run directly
APPIMAGETOOL=/path/to/appimagetool packaging/build_appimage.sh   # build the AppImage
```

## How it works

It uses Roku's [External Control Protocol](https://developer.roku.com/docs/developer-program/dev-tools/external-control-api.md): plain HTTP requests to port 8060 on the device. All of that lives in [`src/ecp.rs`](src/ecp.rs). The UI and a background network thread are in [`src/main.rs`](src/main.rs).

## License

[MIT](LICENSE)
