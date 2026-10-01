<p align="center">
  <img src="brand/gbdev-forge-logo.svg" width="620" alt="gbdev-forge — the wordmark over the tagline 'a code-first game boy toolchain'" />
</p>

A code-first Rust toolchain for building classic Game Boy games now, with Game Boy Advance
support planned later. No drag-and-drop, no node-based visual authoring - real code, a real
built-in run/debug layer, and a shared virtual-hardware model used by both static checks and
live runtime instrumentation.

Design spec: see the firstmate fleet's `data/gbdev-ide-spec/report.md`.

## Features

- **`forge-core`** - shared types (`Tile`, `Palette`, project manifest) used by every other crate.
- **`forge-png2tile`** - PNG to Game Boy 2BPP tile converter, bit-for-bit verified against real
  `rgbgfx` output. Auto-deduplicates tiles across identity/horizontal-flip/vertical-flip/180°, so
  repeated and solid-colour tiles collapse to one, with a before/after tile-count report.
- **`forge-pack`** - map and meta-sprite packer: turns `forge-png2tile`'s tile references into a
  compact tile-ID index array for backgrounds, or a tileId/offsetX/offsetY/flip tuple list for
  multi-tile sprites.
- **`forge-bank`** - MBC (MBC1/3/5) bank allocator: greedily packs bankable items into 16KB banks,
  honours explicit pins, and emits an RGBDS-compatible linker-script fragment. Overflow is a hard
  error naming exactly what didn't fit and by how much - never a silent wrap.
- **`ide/`** - an `egui` desktop application tying the above together in one window:
  - A GB/GBC core (TerminalGB) embedded and running live - play, pause, single-instruction step,
    single-frame step.
  - A disassembly view of the next few real CPU instructions.
  - A memory viewer (WRAM) updated live from the running core.
  - An address read/write watch.
  - A breakpoint list that actually pauses execution when hit.
  - Panels for `forge-png2tile` and `forge-pack`, calling their library code directly rather than
    shelling out.
  - A basic text file editor.
- **`examples/hello-gb`** - a minimal real example (a hand-assembled `.asm` stub plus a
  `forge-png2tile`-generated tile asset) to open and poke at.
- **`docs/getting-started.md`** - how to build, run the IDE, and use the tools above.

## Status

Early build, in active development. The pieces above exist and are tested; there is no released
version and the on-disk project-file format is not yet stable.

## TODO

- Audio driver (`forge-audio`): MIDI/tracker input to raw writes on the Game Boy's four sound
  channels.
- Visual map/collision painter and an NPC/event script DSL compiling to a small bytecode VM,
  layered over the same artifacts the CLI tools already produce.
- The shared virtual-hardware model: a single cartridge + full-address-space model used for BOTH
  build-time fit/overlap checking of declared memory regions AND live runtime violation detection
  (stray writes outside a declared region, dead allocations) - currently only the IDE's raw
  address watch exists; the declared-region/violation layer on top of it is not built yet.
- Game Boy Advance backend (`forge-gba`): a genuinely separate pipeline (4bpp/8bpp tiles, flat
  address space, no classic bank-switching), targeting the `agb` Rust crate as the GBA runtime -
  not started.
- Plugin API: still an open design question - in-process Rust trait vs. a WASM component model for
  untrusted third-party plugins.
- AI integration: still an open design question - likely scope is DSL-script assistance and
  bank-overflow explanations first, nothing further decided.
- Wider IDE polish: resizable/dockable panel layout, execution log/console panel, savestate
  support (TerminalGB already has it, not yet wired into the IDE).
