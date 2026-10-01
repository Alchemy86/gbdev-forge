# Getting started

What actually works in this repo today, and how to use it. Written against
the state of the `ide/` crate after its multi-panel/debugger rework -- if a
panel or button described here doesn't exist, this doc is stale and should be
fixed, not the other way around.

## Build and run the IDE

```sh
cargo build --workspace
cargo test --workspace
cargo run -p forge-ide
```

The IDE is a single `egui`/`eframe` window (`ide/src/main.rs`) built on
[`egui_dock`](https://docs.rs/egui_dock) so every panel below is an
independently resizable, re-dockable tab -- drag a tab's title onto another
panel's edge to split or merge the layout, the way any `egui_dock`-based
app works. The default layout on first launch:

- **Editor** (left) -- a plain text editor for any file (`Open file`/`Save
  file` in the toolbar). No syntax highlighting; it's a text box, not an IDE
  editor widget yet.
- **Emulator** (center top) -- the live Game Boy screen, once a ROM is
  loaded.
- **Disassembly** / **PNG->Tile** (center bottom, tabbed).
- **Registers** (right top) -- PC, IME, ROM bank, HALT-bug latch, frame
  count, host-write count for the loaded ROM.
- **Memory** / **Watch** / **Breakpoints** / **Console** (right bottom,
  tabbed).
- **Disassembly** / **PNG->Tile** / **Pack** / **Bank** (bottom-left,
  tabbed) -- the latter three call `forge-png2tile`/`forge-pack`/
  `forge-bank`'s library functions directly (no subprocess).

See [`usage.md`](usage.md) for a full task-by-task walkthrough with
screenshots of each panel actually running.

## Running a ROM

Toolbar buttons, left to right:

- **Load ROM** -- loads whatever path is typed into the ROM-path field.
- **Load bundled test ROM** -- loads `ide/assets/test-roms/gbselftest.gb`
  (MIT, mirrored from TerminalGB's own `third_party/gbselftest/`), a
  one-click smoke test with no external file needed.
- **Play/Pause** -- runs continuously at the emulator's own frame rate.
- **Step instruction** -- executes exactly one CPU instruction via the
  core's `step_instruction()`.
- **Step frame** -- executes one full ~59.7 Hz frame via `frame()`.
- **Run to breakpoint** -- enabled once at least one breakpoint is set (see
  below); steps instruction-by-instruction until the PC matches a
  breakpoint or a large step cap is hit (so a breakpoint that's never
  reached can't freeze the UI).
- **Save state** / **Load state** -- an in-memory slot backed by the core's
  own `save_state()`/`load_state()` (see TerminalGB's `docs/mcp.md`'s
  `save_state`/`load_state` actions); not yet persisted to a file.

Arrow keys move the D-pad; Z/X are A/B; Enter/Backspace are Start/Select.
Input only applies while the emulator is running (Play, not paused).

## Debugging

- **Registers** shows what the embedded core's public debug surface
  actually exposes today: PC, IME, ROM bank, and the HALT-bug latch -- not
  the full A/B/C/D/E/H/L/SP register file or flags, which aren't on
  TerminalGB's embedding API yet (a core-side change, not an IDE gap; the
  panel says so).
- **Disassembly** decodes the next N instructions from the current PC using
  a small SM83 decoder written for the IDE (`ide/src/disasm.rs`) -- checked
  against TerminalGB's own `docs/debugging.md` first, which documents a
  planned `disassemble()` API but doesn't implement one yet in the pinned
  embedding build, so this isn't duplicating an existing capability. The
  current instruction is marked `->`; any address with a breakpoint is
  marked `*`.
- **Breakpoints** lets you type a hex address and toggle a breakpoint on
  it, or remove one from the list. **Run to breakpoint** (toolbar) then
  steps until the PC hits one, logging the hit to **Console**.
- **Console** is a running execution log (breakpoint hits, state
  load/save, breakpoint set/clear) -- not a REPL, just a log.
- **Memory** is a live hex+ASCII dump of WRAM (`$C000-$DFFF`).
- **Watch** polls an address range frame-over-frame and logs any byte that
  changed since the last sample (the core has no per-address touched-list
  yet, so this is diffing, not a real write-trap -- see
  `ide/src/emulator.rs`'s module docs).

## The PNG->Tile panel

Calls `forge-png2tile`'s library functions directly (no subprocess). Point
it at a PNG whose dimensions are multiples of 8x8 and it converts to Game
Boy 2BPP tile data in-process. The standalone CLI (`cargo run -p
forge-png2tile -- --help`) is the same tool, usable independently of the
IDE -- see its own `--help` output for the full flag set (explicit palette
file, `.c`/`.asm` hex-array output, tile-map JSON with flip/dedup info).

## The `hello-gb` example

`examples/hello-gb/` is a tiny, real, end-to-end example: a hand-drawn
16x16 PNG run through `forge-png2tile`, and a minimal hand-written RGBDS
`.asm` file that copies the resulting tiles into VRAM and displays them.
See `examples/hello-gb/README.md` for the exact commands, what has and
hasn't actually been verified, and a real bug that building this example
caught (an unset LCDC bit left the screen blank). Building it yourself
needs RGBDS, which isn't vendored in this repo -- the README says how to
fetch a prebuilt release.

## Headless sanity checks (no window needed)

Two `ide/examples/` binaries drive the embedded core without opening the
`egui` window -- useful for CI-style checks or verifying a ROM boots before
ever loading it in the GUI:

```sh
# Runs the bundled gbselftest ROM for N frames, asserts the framebuffer
# actually changes frame-to-frame, writes the final frame to a PNG.
cargo run -p forge-ide --example headless_smoke -- 120 /tmp/smoke.png

# Same idea for any ROM file, with a per-frame PC/LY/LCDC trace.
cargo run -p forge-ide --example run_rom -- path/to/rom.gb 10 /tmp/out.png
```

## What's not implemented yet

- No syntax highlighting or symbol-aware navigation in the Editor panel.
- No `.sym`/`.noi` symbol-map loading, so the Disassembly panel shows raw
  addresses, not symbol names.
- No real per-address write-trap (Watch is a frame-over-frame diff, not a
  hardware-style breakpoint on memory access).
- No persistence for save states (in-memory slot only, cleared on IDE
  restart).
