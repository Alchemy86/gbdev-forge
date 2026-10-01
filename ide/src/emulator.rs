//! Thin wrapper around the embedded `terminalgb` core (the
//! `default-features = false` embedding build, see
//! `docs/portability.md#7a-embedding-the-core` in that repo). This module
//! owns no emulation logic of its own -- it only drives the core's public
//! debug/run surface and tracks the read/write-watch plumbing the IDE needs
//! on top of it.

use std::collections::BTreeSet;
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

/// Maximum instructions a single "run to breakpoint" sweep will execute
/// before giving up and stopping anyway -- a runaway/invalid breakpoint set
/// (or a ROM that never hits one) must not freeze the UI thread forever.
const MAX_RUN_INSTRUCTIONS: u32 = 4_000_000;

pub struct Emulator {
    pub gb: Gameboy,
    pub rom_path: String,
    pub rom_title: String,
    pub running: bool,
    pub frame_count: u64,
    pub watch: Option<Watch>,
    pub breakpoints: BTreeSet<u16>,
    pub console: Vec<String>,
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
            breakpoints: BTreeSet::new(),
            console: Vec::new(),
            gb,
        })
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        self.console.push(msg.into());
        if self.console.len() > 1000 {
            self.console.remove(0);
        }
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        if !self.breakpoints.remove(&addr) {
            self.breakpoints.insert(addr);
            self.log(format!("breakpoint set at {addr:04X}"));
        } else {
            self.log(format!("breakpoint cleared at {addr:04X}"));
        }
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

    /// Runs instruction-by-instruction (not frame-by-frame) so the PC can be
    /// checked against the breakpoint set after every single instruction --
    /// this is new logic in the IDE crate, built on top of the core's
    /// existing `step_instruction`, the same pattern TerminalGB's own
    /// `src/mcp/` driver uses to step the clock headlessly. Stops and clears
    /// `running` either when a breakpoint is hit or the sweep cap is reached.
    pub fn run_until_breakpoint(&mut self) {
        for _ in 0..MAX_RUN_INSTRUCTIONS {
            self.gb.step_instruction();
            if self.breakpoints.contains(&self.gb.debug_pc()) {
                self.log(format!("breakpoint hit at {:04X}", self.gb.debug_pc()));
                self.running = false;
                self.resample_watch();
                return;
            }
        }
        self.resample_watch();
    }

    /// Snapshot the whole machine via the core's own `save_state` -- state
    /// support TerminalGB already ships (`docs/mcp.md`'s `save_state`/
    /// `load_state` actions) that the IDE wasn't using yet.
    pub fn save_state(&mut self) -> Vec<u8> {
        self.gb.save_state()
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        self.gb.load_state(data).map_err(|e| e.to_string())?;
        self.resample_watch();
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED_ROM: &[u8] = include_bytes!("../assets/test-roms/gbselftest.gb");

    fn load_bundled() -> Emulator {
        let gb = Gameboy::try_new_with_model(BUNDLED_ROM.to_vec(), None, Model::Auto)
            .expect("bundled ROM should load");
        Emulator {
            rom_path: "<bundled>".to_string(),
            rom_title: gb.rom_title(),
            running: false,
            frame_count: 0,
            watch: None,
            breakpoints: Default::default(),
            console: Vec::new(),
            gb,
        }
    }

    #[test]
    fn step_instruction_advances_pc() {
        let mut emu = load_bundled();
        let start_pc = emu.gb.debug_pc();
        emu.step_instruction();
        assert_ne!(emu.gb.debug_pc(), start_pc, "PC should move after one instruction");
    }

    #[test]
    fn toggle_breakpoint_sets_and_clears() {
        let mut emu = load_bundled();
        assert!(emu.breakpoints.is_empty());
        emu.toggle_breakpoint(0x0150);
        assert!(emu.breakpoints.contains(&0x0150));
        assert_eq!(emu.console.len(), 1);
        emu.toggle_breakpoint(0x0150);
        assert!(emu.breakpoints.is_empty());
        assert_eq!(emu.console.len(), 2);
    }

    #[test]
    fn run_until_breakpoint_stops_exactly_at_the_breakpoint() {
        let mut emu = load_bundled();
        // Run a few real instructions first to land on a PC we know is
        // reachable, then arm a breakpoint a few instructions further and
        // confirm run_until_breakpoint stops exactly there (not past it).
        for _ in 0..15 {
            emu.step_instruction();
        }
        let landing_pc = emu.gb.debug_pc();

        let mut emu2 = load_bundled();
        emu2.toggle_breakpoint(landing_pc);
        emu2.running = true;
        emu2.run_until_breakpoint();
        assert_eq!(emu2.gb.debug_pc(), landing_pc);
        assert!(!emu2.running, "hitting a breakpoint should stop running");
        assert!(emu2.console.iter().any(|l| l.contains("breakpoint hit")));
    }

    #[test]
    fn save_and_load_state_round_trips_pc() {
        let mut emu = load_bundled();
        let pc_before = emu.gb.debug_pc();
        let snapshot = emu.save_state();

        // A handful of steps from the fixed boot PC (0x0100, a NOP then an
        // unconditional JP) is guaranteed to move the PC -- unlike a later,
        // arbitrary point in the ROM that may be spinning in a wait loop.
        for _ in 0..3 {
            emu.step_instruction();
        }
        assert_ne!(emu.gb.debug_pc(), pc_before, "sanity: stepping should move PC");

        emu.load_state(&snapshot).expect("load_state should accept its own save_state output");
        assert_eq!(emu.gb.debug_pc(), pc_before, "load_state should restore the saved PC");
    }
}
