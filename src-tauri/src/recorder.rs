//! G HUB's MR key: record a macro on the fly, straight onto a G-key.
//!
//! MR → press a G-key (the target) → type → MR again saves. MR twice in a row
//! cancels. Keystrokes are read from the keyboard's own evdev node(s) under
//! `/dev/input`, and only between the target being chosen and the second MR
//! press; nothing is read at any other time. Timing between events is kept,
//! as G HUB does.

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::hidpp::onboard::MacroStep;
use crate::keymap;

/// Longest pause kept between two recorded events.
const MAX_GAP_MS: u128 = 5000;

enum Phase {
    Idle,
    /// MR pressed: the next G-key press picks the target.
    Armed { device: String },
    Recording { device: String, button: u8, stop: Arc<AtomicBool>, events: Arc<Mutex<Vec<(Instant, u16, bool)>>> },
}

pub struct Recorder {
    phase: Mutex<Phase>,
}

impl Default for Recorder {
    fn default() -> Self {
        Recorder { phase: Mutex::new(Phase::Idle) }
    }
}

/// What the caller should do after an MR or G-key edge.
pub enum Outcome {
    /// Nothing to do; the edge was not the recorder's.
    Ignored,
    /// The recorder used the edge (the key's own action must not run).
    Consumed,
    /// MR pressed while idle: light the LED.
    Armed,
    /// A second MR press before a target: put the LED out.
    Cancelled,
    /// Finished: bind these steps to `button` on `device`.
    Finished { device: String, button: u8, steps: Vec<MacroStep> },
}

impl Recorder {
    /// MR pressed on `device`.
    pub fn mr_pressed(&self, device: &str) -> Outcome {
        let mut phase = self.phase.lock();
        match std::mem::replace(&mut *phase, Phase::Idle) {
            Phase::Idle => {
                *phase = Phase::Armed { device: device.to_string() };
                Outcome::Armed
            }
            Phase::Armed { .. } => Outcome::Cancelled,
            Phase::Recording { device, button, stop, events } => {
                stop.store(false, Ordering::SeqCst);
                let steps = to_steps(&events.lock());
                Outcome::Finished { device, button, steps }
            }
        }
    }

    /// A G-key edge on `device`. While armed, the first press picks the
    /// target and starts reading keystrokes.
    pub fn gkey(&self, device: &str, button: u8, pressed: bool, input_name: &str) -> Outcome {
        let mut phase = self.phase.lock();
        match &*phase {
            Phase::Armed { device: d } if d == device => {
                if !pressed {
                    return Outcome::Consumed;
                }
                let stop = Arc::new(AtomicBool::new(true));
                let events = Arc::new(Mutex::new(Vec::new()));
                spawn_readers(input_name, Arc::clone(&stop), Arc::clone(&events));
                *phase = Phase::Recording { device: device.to_string(), button, stop, events };
                Outcome::Consumed
            }
            // The target's release, right after it was picked.
            Phase::Recording { device: d, button: b, .. } if d == device && *b == button && !pressed => Outcome::Consumed,
            _ => Outcome::Ignored,
        }
    }
}

/// Reads key events from every readable `/dev/input` node whose name
/// contains `name` (a wired G915 exposes several), until `run` clears.
fn spawn_readers(name: &str, run: Arc<AtomicBool>, events: Arc<Mutex<Vec<(Instant, u16, bool)>>>) {
    let Ok(dir) = std::fs::read_dir("/sys/class/input") else { return };
    let mut opened = 0;
    for entry in dir.flatten() {
        let node = entry.file_name().to_string_lossy().into_owned();
        if !node.starts_with("event") {
            continue;
        }
        let dev_name = std::fs::read_to_string(entry.path().join("device/name")).unwrap_or_default();
        if !dev_name.contains(name) {
            continue;
        }
        let Ok(mut file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(format!("/dev/input/{node}"))
        else {
            continue;
        };
        opened += 1;
        let run = Arc::clone(&run);
        let events = Arc::clone(&events);
        std::thread::spawn(move || read_loop(&mut file, &run, &events));
    }
    if opened == 0 {
        log::warn!("MR: no readable /dev/input node for '{name}'");
    }
}

fn read_loop(file: &mut File, run: &AtomicBool, events: &Mutex<Vec<(Instant, u16, bool)>>) {
    // struct input_event on 64-bit: timeval (16), type u16, code u16, value i32.
    let mut buf = [0u8; 24];
    while run.load(Ordering::SeqCst) {
        match file.read(&mut buf) {
            Ok(24) => {
                let kind = u16::from_ne_bytes([buf[16], buf[17]]);
                let code = u16::from_ne_bytes([buf[18], buf[19]]);
                let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
                // EV_KEY press / release; auto-repeat (2) is not recorded.
                if kind == 1 && (value == 0 || value == 1) {
                    events.lock().push((Instant::now(), code, value == 1));
                }
            }
            _ => std::thread::sleep(Duration::from_millis(3)),
        }
    }
}

/// Recorded key events → macro steps, with the pauses between them.
fn to_steps(events: &[(Instant, u16, bool)]) -> Vec<MacroStep> {
    let mut steps = Vec::new();
    let mut last: Option<Instant> = None;
    for (at, code, down) in events {
        let step = if let Some(bit) = keymap::modifier_bit_for_code(*code) {
            if *down { MacroStep::ModifiersDown { mask: bit } } else { MacroStep::ModifiersUp { mask: bit } }
        } else if let Some(usage) = keymap::usage_for_code(*code) {
            if *down { MacroStep::KeyDown { usage } } else { MacroStep::KeyUp { usage } }
        } else {
            continue;
        };
        if let Some(prev) = last {
            let gap = at.duration_since(prev).as_millis().min(MAX_GAP_MS) as u16;
            if gap > 0 {
                steps.push(MacroStep::Delay { ms: gap });
            }
        }
        last = Some(*at);
        steps.push(step);
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_events_become_timed_steps() {
        let t = Instant::now();
        let ev = [
            (t, keymap::KEY_LEFTSHIFT, true),
            (t + Duration::from_millis(40), 30, true), // a
            (t + Duration::from_millis(90), 30, false),
            (t + Duration::from_millis(90), keymap::KEY_LEFTSHIFT, false),
        ];
        let steps = to_steps(&ev);
        assert_eq!(steps[0], MacroStep::ModifiersDown { mask: 0x02 });
        assert_eq!(steps[1], MacroStep::Delay { ms: 40 });
        assert_eq!(steps[2], MacroStep::KeyDown { usage: 0x04 });
        assert_eq!(steps[4], MacroStep::KeyUp { usage: 0x04 });
        assert_eq!(steps[5], MacroStep::ModifiersUp { mask: 0x02 });
    }
}
