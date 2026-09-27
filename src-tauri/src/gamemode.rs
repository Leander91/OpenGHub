//! Game Mode, done by OpenGHub itself.
//!
//! The G915's `0x4522` accepts key lists but disables nothing we could
//! observe, so Game Mode grabs the keyboard's key-bearing evdev nodes
//! (`EVIOCGRAB`: only OpenGHub receives their events) and passes every key on
//! through the virtual keyboard — except the disabled ones.
//!
//! The kernel drops the grab when the process exits, so a crash can never
//! leave the keyboard dead. Known limit: while grabbed, lock-key LEDs (Caps
//! Lock) follow the virtual keyboard, not the physical one.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::keymap;
use crate::remap::Injector;

/// `_IOW('E', 0x90, int)`.
const EVIOCGRAB: libc::c_ulong = 0x4004_4590;
const EV_KEY: u16 = 1;

/// HID keyboard usages → evdev key codes (modifiers included).
pub fn codes_for_usages(usages: &[u8]) -> Vec<u16> {
    usages
        .iter()
        .flat_map(|&u| {
            if (0xe0..=0xe7).contains(&u) {
                keymap::modifier_codes(1 << (u - 0xe0))
            } else {
                keymap::usage_code(u).into_iter().collect()
            }
        })
        .collect()
}

struct Grab {
    run: Arc<AtomicBool>,
    blocked: Arc<Mutex<Vec<u16>>>,
}

#[derive(Default)]
pub struct GameMode {
    grabs: Mutex<HashMap<String, Grab>>,
}

impl GameMode {
    /// Turns Game Mode on for `device` (evdev name containing `name`) with
    /// these blocked key codes, or updates the list when already on.
    pub fn enable(&self, device: &str, name: &str, blocked: Vec<u16>, injector: Arc<Injector>) {
        let mut grabs = self.grabs.lock();
        if let Some(g) = grabs.get(device) {
            *g.blocked.lock() = blocked;
            return;
        }
        let run = Arc::new(AtomicBool::new(true));
        let list = Arc::new(Mutex::new(blocked));
        let mut opened = 0;
        for path in key_nodes(name) {
            let Ok(file) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&path) else {
                continue;
            };
            // SAFETY: plain ioctl on an fd we own.
            if unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGRAB, 1 as libc::c_int) } != 0 {
                log::warn!("game mode: could not grab {path}");
                continue;
            }
            opened += 1;
            let (run, list, injector) = (Arc::clone(&run), Arc::clone(&list), Arc::clone(&injector));
            std::thread::Builder::new()
                .name("game-mode".into())
                .spawn(move || forward(file, &run, &list, &injector))
                .ok();
        }
        if opened == 0 {
            log::warn!("game mode: no keyboard node found for '{name}'");
            return;
        }
        log::info!("game mode on for {device} ({opened} node(s))");
        grabs.insert(device.to_string(), Grab { run, blocked: list });
    }

    pub fn disable(&self, device: &str) {
        if let Some(g) = self.grabs.lock().remove(device) {
            g.run.store(false, Ordering::SeqCst);
            log::info!("game mode off for {device}");
        }
    }

    pub fn disable_all(&self) {
        for (_, g) in self.grabs.lock().drain() {
            g.run.store(false, Ordering::SeqCst);
        }
    }
}

/// The evdev nodes of a keyboard that carry ordinary keys (KEY_A), by name.
fn key_nodes(name: &str) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir("/sys/class/input") else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| {
            let node = e.file_name().to_string_lossy().into_owned();
            if !node.starts_with("event") {
                return None;
            }
            let dev_name = std::fs::read_to_string(e.path().join("device/name")).unwrap_or_default();
            let caps = std::fs::read_to_string(e.path().join("device/capabilities/key")).unwrap_or_default();
            (dev_name.contains(name) && has_key(&caps, 30)).then(|| format!("/dev/input/{node}"))
        })
        .collect()
}

/// Whether a sysfs key-capability bitmap (hex words, most significant first)
/// has bit `code` set.
fn has_key(caps: &str, code: usize) -> bool {
    let words: Vec<&str> = caps.split_whitespace().collect();
    let bits = std::mem::size_of::<libc::c_ulong>() * 8;
    let (word, bit) = (code / bits, code % bits);
    words
        .len()
        .checked_sub(word + 1)
        .and_then(|i| u64::from_str_radix(words[i], 16).ok())
        .is_some_and(|w| w >> bit & 1 == 1)
}

fn forward(mut file: File, run: &AtomicBool, blocked: &Mutex<Vec<u16>>, injector: &Injector) {
    // struct input_event on 64-bit: timeval (16), type u16, code u16, value i32.
    let mut buf = [0u8; 24];
    while run.load(Ordering::SeqCst) {
        match file.read(&mut buf) {
            Ok(24) => {
                let kind = u16::from_ne_bytes([buf[16], buf[17]]);
                let code = u16::from_ne_bytes([buf[18], buf[19]]);
                let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
                // Presses and releases pass on; the compositor makes its own
                // repeats, so the kernel's (value 2) are not needed.
                if kind == EV_KEY && value != 2 && !blocked.lock().contains(&code) {
                    if let Err(e) = injector.key(code, value == 1) {
                        log::warn!("game mode: key not passed on: {e}");
                    }
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Sleep until the keyboard sends something (or 100 ms pass, to
                // notice being switched off).
                let mut pfd = libc::pollfd { fd: file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                // SAFETY: one valid pollfd.
                unsafe { libc::poll(&mut pfd, 1, 100) };
            }
            Err(_) => break, // unplugged
        }
    }
    // SAFETY: as above; the grab also ends when the fd closes.
    unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGRAB, 0 as libc::c_int) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usages_map_to_evdev_codes() {
        assert_eq!(codes_for_usages(&[0x04, 0xe3, 0x65]), vec![30, keymap::KEY_LEFTMETA, 127]);
    }

    #[test]
    fn reads_key_capability_bitmaps() {
        // Bit 30 (KEY_A) in the lowest word.
        assert!(has_key("0 0 40000000", 30));
        assert!(!has_key("0 0 0", 30));
    }
}
