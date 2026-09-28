//! Keyboard animations (G HUB's LIGHTSYNC ANIMATIONS): a sequence of
//! Freestyle frames, played by OpenGHub by writing per-key colours.
//!
//! Each frame is shown for its duration; a "fade" transition blends into the
//! next one in a few steps. Only keys whose colour changed are written after
//! the first frame, which keeps a wireless keyboard responsive.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use tauri::Manager;

use crate::state::DeviceManager;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    /// LED id (as a string key) → `#rrggbb`; missing keys are off.
    #[serde(default)]
    pub keys: HashMap<String, String>,
    /// How long this frame shows, in ms.
    #[serde(default = "default_frame_ms")]
    pub duration_ms: u32,
}

fn default_frame_ms() -> u32 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Animation {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub frames: Vec<Frame>,
    /// `cycle` | `reverse` | `bounce` | `random`
    #[serde(default = "default_cycle")]
    pub cycle: String,
    /// `none` | `fade`
    #[serde(default = "default_transition")]
    pub transition: String,
}

fn default_cycle() -> String {
    "cycle".into()
}
fn default_transition() -> String {
    "fade".into()
}

type Colours = HashMap<u8, [u8; 3]>;

fn colours(frame: &Frame) -> Colours {
    frame
        .keys
        .iter()
        .filter_map(|(led, hex)| Some((led.parse().ok()?, crate::lightsync::parse_hex(hex))))
        .collect()
}

/// Frame indices in play order, for one pass.
fn order(a: &Animation, pass: u64) -> Vec<usize> {
    let n = a.frames.len();
    match a.cycle.as_str() {
        "reverse" => (0..n).rev().collect(),
        "bounce" if n > 2 => (0..n).chain((1..n - 1).rev()).collect(),
        "random" => {
            // A cheap, deterministic shuffle per pass; no RNG dependency.
            let mut v: Vec<usize> = (0..n).collect();
            let mut x = pass.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
            for i in (1..n).rev() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                v.swap(i, (x % (i as u64 + 1)) as usize);
            }
            v
        }
        _ => (0..n).collect(),
    }
}

fn blend(a: &Colours, b: &Colours, t: f32) -> Colours {
    let mut out = Colours::new();
    for led in a.keys().chain(b.keys()) {
        let ca = a.get(led).copied().unwrap_or([0; 3]);
        let cb = b.get(led).copied().unwrap_or([0; 3]);
        let mix = |i: usize| (ca[i] as f32 + (cb[i] as f32 - ca[i] as f32) * t).round() as u8;
        out.insert(*led, [mix(0), mix(1), mix(2)]);
    }
    out
}

/// One running animation per device; starting another stops the last.
#[derive(Default)]
pub struct Animator {
    /// Bumped to stop the device's current loop.
    generation: Mutex<HashMap<String, Arc<AtomicU64>>>,
    running: Mutex<HashMap<String, Animation>>,
}

impl Animator {
    pub fn play(&self, app: tauri::AppHandle, device: &str, anim: Animation) {
        if self.running.lock().get(device) == Some(&anim) {
            return; // already showing exactly this
        }
        self.stop(device);
        if anim.frames.is_empty() {
            return;
        }
        let gen = Arc::clone(self.generation.lock().entry(device.to_string()).or_default());
        let mine = gen.load(Ordering::SeqCst);
        self.running.lock().insert(device.to_string(), anim.clone());
        let device = device.to_string();
        std::thread::Builder::new()
            .name("animation".into())
            .spawn(move || {
                let manager = app.state::<DeviceManager>();
                let alive = || gen.load(Ordering::SeqCst) == mine;
                let mut shown: Option<Colours> = None;
                let mut show = |next: Colours| {
                    let first = shown.is_none();
                    let mut diff: Vec<(u8, [u8; 3])> = next
                        .iter()
                        .filter(|(led, c)| shown.as_ref().and_then(|s| s.get(led)) != Some(c))
                        .map(|(l, c)| (*l, *c))
                        .collect();
                    // Keys lit before and absent now go dark.
                    if let Some(prev) = &shown {
                        diff.extend(prev.keys().filter(|l| !next.contains_key(l)).map(|l| (*l, [0; 3])));
                    }
                    if first || !diff.is_empty() {
                        if let Err(e) = manager.set_per_key_frame(&device, &diff, first) {
                            log::debug!("animation frame not written: {e}");
                        }
                    }
                    shown = Some(next);
                };
                let mut pass = 0u64;
                while alive() {
                    let seq = order(&anim, pass);
                    for (i, &f) in seq.iter().enumerate() {
                        if !alive() {
                            return;
                        }
                        let frame = &anim.frames[f];
                        let here = colours(frame);
                        let hold = Duration::from_millis(frame.duration_ms.clamp(20, 60_000) as u64);
                        if anim.transition == "fade" {
                            let next = colours(&anim.frames[seq[(i + 1) % seq.len()]]);
                            // A handful of steps: smooth enough, light on the radio.
                            let steps = ((hold.as_millis() / 60) as usize).clamp(1, 8);
                            for s in 0..steps {
                                if !alive() {
                                    return;
                                }
                                show(blend(&here, &next, s as f32 / steps as f32));
                                std::thread::sleep(hold / steps as u32);
                            }
                        } else {
                            show(here);
                            std::thread::sleep(hold);
                        }
                    }
                    pass += 1;
                }
            })
            .ok();
    }

    pub fn stop(&self, device: &str) {
        if let Some(g) = self.generation.lock().get(device) {
            g.fetch_add(1, Ordering::SeqCst);
        }
        self.running.lock().remove(device);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(cycle: &str, n: usize) -> Animation {
        Animation {
            name: String::new(),
            frames: (0..n).map(|_| Frame { keys: HashMap::new(), duration_ms: 100 }).collect(),
            cycle: cycle.into(),
            transition: "none".into(),
        }
    }

    #[test]
    fn play_orders() {
        assert_eq!(order(&anim("cycle", 3), 0), vec![0, 1, 2]);
        assert_eq!(order(&anim("reverse", 3), 0), vec![2, 1, 0]);
        assert_eq!(order(&anim("bounce", 4), 0), vec![0, 1, 2, 3, 2, 1]);
        let mut r = order(&anim("random", 5), 7);
        r.sort();
        assert_eq!(r, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn fade_halfway() {
        let a: Colours = [(1, [0, 0, 0])].into();
        let b: Colours = [(1, [200, 100, 50])].into();
        assert_eq!(blend(&a, &b, 0.5)[&1], [100, 50, 25]);
    }
}
