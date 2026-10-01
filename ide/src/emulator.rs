//! Thin wrapper around the embedded `terminalgb` core (the
//! `default-features = false` embedding build, see
//! `docs/portability.md#7a-embedding-the-core` in that repo). This module
//! owns no emulation logic of its own -- it only drives the core's public
//! debug/run surface and tracks the read/write-watch plumbing the IDE needs
//! on top of it.

use std::path::Path;

use terminalgb::gameboy::Gameboy;
use terminalgb::mode::Model;
use terminalgb::KeypadKey;

pub const WRAM_START: u16 = 0xC000;
pub const WRAM_LEN: usize = 0x2000;

/// One change observed in a watched address range between two consecutive
/// frames.
pub struct WatchEvent {
    pub frame: u64,
    pub address: u16,
    pub old: u8,
    pub new: u8,
}

/// A user-defined address range being watched for writes. The core has no
/// per-address touched-list, so this watches by diffing `peek_range` output
/// frame over frame -- any byte that changed since the last sample implies a
/// write landed there in between. That's the "basic plumbing" this pass
/// needs; a real touched-address feed from the core is a follow-up.
pub struct Watch {
    pub start: u16,
    pub len: usize,
    last_sample: Vec<u8>,
    pub counts: Vec<u64>,
    pub log: Vec<WatchEvent>,
}

impl Watch {
    pub fn new(start: u16, len: usize, initial: Vec<u8>) -> Self {
        let counts = vec![0u64; len];
        Self {
            start,
            len,
            last_sample: initial,
            counts,
            log: Vec::new(),
        }
    }

    fn sample(&mut self, frame: u64, current: &[u8]) {
        for (i, (&prev, &now)) in self.last_sample.iter().zip(current.iter()).enumerate() {
            if prev != now {
                self.counts[i] = self.counts[i].saturating_add(1);
                self.log.push(WatchEvent {
                    frame,
                    address: self.start.wrapping_add(i as u16),
                    old: prev,
                    new: now,
                });
                if self.log.len() > 500 {
                    self.log.remove(0);
                }
            }
        }
        self.last_sample.copy_from_slice(current);
    }

    pub fn total_writes(&self) -> u64 {
        self.counts.iter().sum()
    }
}

pub struct Emulator {
    pub gb: Gameboy,
    pub rom_path: String,
    pub rom_title: String,
    pub running: bool,
    pub frame_count: u64,
    pub watch: Option<Watch>,
}

impl Emulator {
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let gb = Gameboy::try_new_with_model(data, None, Model::Auto)?;
        let rom_title = gb.rom_title();
        Ok(Self {
            rom_path: path.display().to_string(),
            rom_title,
            running: false,
            frame_count: 0,
            watch: None,
            gb,
        })
    }

    /// One full 59.7275 Hz frame -- `Gameboy::frame()`, matching the
    /// embedding example in `docs/portability.md#7a-embedding-the-core`.
    pub fn step_frame(&mut self) {
        self.gb.frame();
        self.frame_count += 1;
        self.resample_watch();
    }

    /// One CPU instruction, via the core's own `step_instruction`.
    pub fn step_instruction(&mut self) {
        self.gb.step_instruction();
        self.resample_watch();
    }

    pub fn start_watch(&mut self, start: u16, len: usize) {
        let initial = self.gb.peek_range(start, len);
        self.watch = Some(Watch::new(start, len, initial));
    }

    pub fn clear_watch(&mut self) {
        self.watch = None;
    }

    fn resample_watch(&mut self) {
        if let Some(watch) = &mut self.watch {
            let current = self.gb.peek_range(watch.start, watch.len);
            watch.sample(self.frame_count, &current);
        }
    }

    pub fn wram_dump(&mut self) -> Vec<u8> {
        self.gb.peek_range(WRAM_START, WRAM_LEN)
    }

    pub fn key_down(&mut self, key: KeypadKey) {
        self.gb.keydown(key);
    }

    pub fn key_up(&mut self, key: KeypadKey) {
        self.gb.keyup(key);
    }
}
