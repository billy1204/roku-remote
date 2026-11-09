# Roku Remote

A small desktop remote for Roku TVs and streaming players on your home network, for Linux (AppImage) and Windows (.exe). Written in Rust with [egui](https://github.com/emilk/egui).

> **Unofficial.** This is an independent project, not affiliated with, endorsed by or sponsored by Roku, Inc. Roku is a trademark of Roku, Inc.; YouTube is a trademark of Google LLC.

<img src="assets/icon.svg" width="64" alt="Roku Remote icon">

## Features

- **Finds your Rokus automatically** (SSDP), or connect by IP address. Remembers the last device.
- **Full remote:** Power, Home, Back, Options, D-pad and OK, Replay, Rewind, Play/Pause, Fast-forward.
- **TV controls** (Roku TVs only): Volume up/down/mute and HDMI 1–3/Antenna inputs.
- **Type into search boxes** from your keyboard instead of the on-screen keyboard.
- **Channel launcher:** a button for every installed channel.
- **Now playing:** the current channel, play state and progress, in the window and the title bar.
- **Play YouTube links:** paste a YouTube link (watch, youtu.be, Shorts or live) with **Ctrl+V** and the TV's YouTube channel opens and plays it. (Right-click paste doesn't work in the app's text boxes.)
- **Share media:** pick a folder and play its videos, music and photos on the TV with Roku's free **Roku Media Player** channel. The **?** button explains the setup, checks that Media Player is on the TV and warns if a firewall may keep the TV out. The tray can start and stop sharing too.
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

If a TV is still on Limited, the app tells you when you connect. The **?** button at the top of the app shows these steps too.

## Run it

Download from the [latest release](https://github.com/billy1204/roku-remote/releases/latest), or build it yourself (below).

**Linux:** download `Roku-Remote-x86_64.AppImage`, then:

```bash
chmod +x Roku-Remote-x86_64.AppImage
./Roku-Remote-x86_64.AppImage
```

**Windows:** download `Roku-Remote-windows-x86_64.exe` and double-click it. It's not code-signed, so Windows SmartScreen may warn you the first time; choose **More info > Run anyway**. If Windows Firewall asks, allow it on private networks so it can find your Rokus.

Your computer and the Roku must be on the same network.

### Sharing media

Install **Roku Media Player** on the TV (Home > Streaming Store > Roku Media Player > Add channel). Then click **Share a folder…** in the app. Media Player opens on the TV; choose **Roku Remote (*your computer's name*)** and browse with the Roku remote.

The share is read-only, and only the folder you pick is visible. It answers only devices on your local network, never the internet, but anyone on the *same* network can browse it while it's shared (DLNA has no passwords). Share at home, not on public Wi-Fi, and press **Stop** when you're done.

If the TV can't see your computer, a firewall may be blocking it. The app detects ufw, firewalld and Windows Firewall and shows how to let the TV in (TCP ports 8200–8209 and UDP port 1900).

## Build

Requires a Rust toolchain (edition 2024) and [appimagetool](https://github.com/AppImage/appimagetool).

```bash
cargo run --release                       # run directly
cargo test                                # run the tests
APPIMAGETOOL=/path/to/appimagetool packaging/build_appimage.sh   # build the AppImage
```

After changing dependencies, run `packaging/licenses.sh` (needs `cargo install cargo-about --locked --features cli`) to regenerate [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md).

On Windows, `cargo build --release` produces `target\release\roku-remote.exe`. Publishing a GitHub release also builds the `.exe` and attaches it automatically ([workflow](.github/workflows/windows.yml)).

## How it works

It uses Roku's [External Control Protocol](https://developer.roku.com/docs/developer-program/dev-tools/external-control-api.md): plain HTTP requests to port 8060 on the device, which can press keys, launch channels (including YouTube with a video), and report what's playing.

| File | What's in it |
|---|---|
| [`src/main.rs`](src/main.rs) | The window: remote, now playing, YouTube box, sharing card and help panels |
| [`src/core.rs`](src/core.rs) | The background worker and state that outlive the window: network commands, the now-playing poll, saved settings |
| [`src/ecp.rs`](src/ecp.rs) | Talking to the Roku: discovery (SSDP), keys, channels, now playing, YouTube links |
| [`src/share.rs`](src/share.rs) | The media share: a small read-only DLNA server for Roku Media Player, and the firewall check |
| [`src/pick.rs`](src/pick.rs) | The folder picker (the desktop portal on Linux, the system dialog on Windows) |
| [`src/tray.rs`](src/tray.rs), [`src/mpris.rs`](src/mpris.rs) | Linux only: the tray icon, and desktop media controls over D-Bus |
| [`src/look.rs`](src/look.rs) | Colours, icons and the remote's buttons |

## Contributing

Bug reports and pull requests are welcome. Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE). The app includes open-source libraries, fonts, Rust's standard library and (in the AppImage) the AppImage runtime, under their own licenses; their notices are in [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md), which also ships inside the AppImage and with each release.

Roku is a trademark of Roku, Inc., and YouTube a trademark of Google LLC, used here only to say what the app works with. This project is not affiliated with or endorsed by either company.
