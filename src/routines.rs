//! One-click routines: a named list of steps such as TV on, HDMI 1,
//! volume 20.
use crate::ecp;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// One of `KEYS`.
    Key(&'static str),
    /// Open a channel: its id and name.
    Launch(String, String),
    Volume(u32),
    /// Start the sleep timer, in minutes.
    Sleep(u32),
    /// Pause, in seconds.
    Wait(u32),
}

/// The keys a routine can press, and how they read.
pub const KEYS: [(&str, &str); 8] = [
    ("PowerOn", "TV on"),
    ("PowerOff", "TV off"),
    ("InputHDMI1", "HDMI 1"),
    ("InputHDMI2", "HDMI 2"),
    ("InputHDMI3", "HDMI 3"),
    ("InputHDMI4", "HDMI 4"),
    ("InputAV1", "AV"),
    ("InputTuner", "Antenna"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Routine {
    pub name: String,
    pub steps: Vec<Step>,
}

impl Step {
    pub fn label(&self) -> String {
        match self {
            Step::Key(key) => KEYS.iter().find(|(k, _)| k == key).map_or(*key, |(_, l)| *l).to_string(),
            Step::Launch(_, name) => format!("Open {name}"),
            Step::Volume(v) => format!("Volume {v}"),
            Step::Sleep(m) => format!("Sleep timer {m} min"),
            Step::Wait(s) => format!("Wait {s} s"),
        }
    }

    fn line(&self) -> String {
        match self {
            Step::Key(key) => format!("key {key}"),
            Step::Launch(id, name) => format!("launch {id} {}", name.replace('\n', " ")),
            Step::Volume(v) => format!("volume {v}"),
            Step::Sleep(m) => format!("sleep {m}"),
            Step::Wait(s) => format!("wait {s}"),
        }
    }

    fn parse(line: &str) -> Option<Self> {
        let (kind, rest) = line.split_once(' ')?;
        let number = |max: u32| rest.trim().parse::<u32>().ok().filter(|n| *n <= max);
        match kind {
            "key" => KEYS.iter().find(|(k, _)| *k == rest.trim()).map(|(k, _)| Step::Key(k)),
            "launch" => {
                let (id, name) = rest.split_once(' ').unwrap_or((rest, ""));
                let valid = !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_');
                valid.then(|| Step::Launch(id.to_string(), name.trim().to_string()))
            }
            "volume" => number(100).map(Step::Volume),
            "sleep" => number(24 * 60).map(Step::Sleep),
            "wait" => number(60).map(Step::Wait),
            _ => None,
        }
    }
}

/// Routines as saved: a `[name]` line, then a line per step. Lines that
/// don't parse are dropped.
pub fn parse(text: &str) -> Vec<Routine> {
    let mut routines: Vec<Routine> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            routines.push(Routine { name: name.to_string(), steps: Vec::new() });
        } else if let (Some(routine), Some(step)) = (routines.last_mut(), Step::parse(line)) {
            routine.steps.push(step);
        }
    }
    routines
}

pub fn format(routines: &[Routine]) -> String {
    let mut text = String::new();
    for r in routines {
        text.push_str(&format!("[{}]\n", r.name.replace('\n', " ")));
        for step in &r.steps {
            text.push_str(&step.line());
            text.push('\n');
        }
    }
    text
}

/// Run the steps in order, stopping at the first that fails. `sleep` starts
/// the sleep timer, which belongs to the running app.
pub fn run(host: &str, steps: &[Step], sleep: &dyn Fn(u32)) -> Result<(), String> {
    for step in steps {
        match step {
            Step::Key(key) => {
                ecp::keypress(host, key)?;
                if *key == "PowerOn" {
                    // A TV that was off needs a moment before it takes the rest.
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
            Step::Launch(id, _) => ecp::launch(host, id)?,
            Step::Volume(level) => ecp::set_volume(host, *level)?,
            Step::Sleep(minutes) => sleep(*minutes),
            Step::Wait(seconds) => std::thread::sleep(Duration::from_secs(*seconds as u64)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Routine, Step, format, parse};

    #[test]
    fn round_trip() {
        let routines = vec![
            Routine {
                name: "Movie night".into(),
                steps: vec![Step::Key("PowerOn"), Step::Key("InputHDMI1"), Step::Launch("12".into(), "Netflix & more".into()), Step::Volume(20)],
            },
            Routine { name: "Bedtime [quiet]".into(), steps: vec![Step::Volume(8), Step::Wait(5), Step::Sleep(60)] },
            Routine { name: "Empty".into(), steps: Vec::new() },
        ];
        assert_eq!(parse(&format(&routines)), routines);
    }

    #[test]
    fn bad_lines_are_dropped() {
        let text = "volume 20\n[A]\nkey Lit_x\nkey PowerOff\nlaunch ../x Evil\nvolume 500\nnonsense\nwait 5\n";
        assert_eq!(parse(text), vec![Routine { name: "A".into(), steps: vec![Step::Key("PowerOff"), Step::Wait(5)] }]);
    }
}
