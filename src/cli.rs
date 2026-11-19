//! One-shot commands such as `sofa-remote --key Mute`, for binding to the
//! desktop's keyboard shortcuts. They go straight to the TV connected last;
//! the app needn't be running.
use crate::{core, ecp};

const KEYS: [&str; 30] = [
    "Home", "Back", "Select", "Up", "Down", "Left", "Right", "Play", "Rev", "Fwd", "InstantReplay", "Info", "Backspace", "Enter", "Search",
    "VolumeUp", "VolumeDown", "VolumeMute", "PowerToggle", "PowerOn", "PowerOff", "ChannelUp", "ChannelDown", "InputHDMI1", "InputHDMI2",
    "InputHDMI3", "InputHDMI4", "InputAV1", "InputTuner", "FindRemote",
];
const ALIASES: [(&str, &str); 4] = [("Mute", "VolumeMute"), ("Power", "PowerToggle"), ("OK", "Select"), ("PlayPause", "Play")];

const HELP: &str = "Sofa Remote

  sofa-remote                 open the remote
  sofa-remote --hidden        start in the tray
  sofa-remote --key NAME      press a key on the TV connected last
  sofa-remote --volume 0-100  set the TV's volume
  sofa-remote --launch ID     open a channel by its id

Keys: Home Back Select Up Down Left Right Play Rev Fwd InstantReplay Info
Backspace Enter Search VolumeUp VolumeDown Mute Power PowerOn PowerOff
ChannelUp ChannelDown InputHDMI1-4 InputAV1 InputTuner FindRemote";

/// The key's real name, whatever case it was typed in.
fn key_name(name: &str) -> Option<&'static str> {
    let same = |k: &str| k.eq_ignore_ascii_case(name);
    ALIASES.iter().find(|(a, _)| same(a)).map(|(_, k)| *k).or_else(|| KEYS.iter().find(|k| same(k)).copied())
}

/// Run a command-line request. `None` when there is none and the window
/// should open; otherwise the exit code.
pub fn run(args: &[String]) -> Option<i32> {
    let cmd = args.first()?.as_str();
    let value = args.get(1).map(String::as_str);
    let host = || core::saved_host().ok_or("No TV yet: open Sofa Remote and connect to one first".to_string());
    let result = match (cmd, value) {
        ("--hidden", _) => return None,
        ("--help" | "-h", _) => {
            println!("{HELP}");
            return Some(0);
        }
        ("--key", Some(name)) => match key_name(name) {
            Some(key) => host().and_then(|h| ecp::keypress(&h, key)),
            None => Err(format!("unknown key {name}; see --help")),
        },
        ("--volume", Some(level)) => match level.parse::<u32>() {
            Ok(level) if level <= 100 => host().and_then(|h| ecp::set_volume(&h, level)),
            _ => Err("the volume is a number from 0 to 100".into()),
        },
        ("--launch", Some(id)) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_') => {
            host().and_then(|h| ecp::launch(&h, id))
        }
        _ => Err("unknown command; see --help".into()),
    };
    Some(match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sofa-remote: {e}");
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::key_name;

    #[test]
    fn key_names() {
        assert_eq!(key_name("mute"), Some("VolumeMute"));
        assert_eq!(key_name("VOLUMEUP"), Some("VolumeUp"));
        assert_eq!(key_name("inputhdmi2"), Some("InputHDMI2"));
        assert_eq!(key_name("Lit_a"), None);
        assert_eq!(key_name("../launch/1"), None);
    }
}
