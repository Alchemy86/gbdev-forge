# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Add durable project-specific notes here as they are discovered through real work.

## Design spec

Authoritative architecture/roadmap document: the firstmate fleet's
`data/gbdev-ide-spec/report.md` (not in this repo). Follow its contracts
exactly rather than redesigning them; it has already been verified against
`rgbgfx`'s real output format.

## Workspace layout

Cargo workspace, `crates/` for shared library code, `cli/` for standalone
binaries - each CLI tool must work without the others or a GUI shell
(see the design spec's section 4.1 for the full intended shape as more
tools land). `crates/forge-core` has the shared `Tile`/`Palette`/dedup
model; keep it target-agnostic (no GBC- vs GBA-specific logic) so later
tools (debugger, GBA backend) can reuse it without rework.

## Verifying 2BPP output against `rgbgfx`

`rgbgfx` is not preinstalled in this sandbox. It can be fetched without a
package manager: download `rgbds-linux-x86_64.tar.xz` from the latest
release at `https://github.com/gbdev/rgbds/releases/latest` and extract it;
no build step needed. To get a byte-for-byte comparable reference, pin an
explicit palette on both sides so color index assignment lines up -
`rgbgfx -c '#ffffff,#aaaaaa,#555555,#000000' -o ref.2bpp input.png` paired
with a `forge-png2tile --palette` file listing the same 4 colors in the
same order. `forge-png2tile`'s own tile-slicing + 2BPP byte packing has
been verified bit-for-bit against this (both with an explicit palette and
with its auto-quantize path, which happened to pick the same brightness-
descending order rgbgfx's own auto-detection uses for this test image -
not a documented rgbgfx guarantee, so don't rely on auto-quantize matching
rgbgfx auto-detection in general; use `--palette` when exact parity
matters). See `cli/forge-png2tile/tests/conversion.rs`'s
`two_bpp_output_matches_real_rgbgfx_reference` test and its fixture files
under `cli/forge-png2tile/tests/fixtures/`.

## Smart tile dedup

`forge_core::dedup_tiles` merges source tiles that are identical under
flip (the only hardware-free transform - sprite OAM X/Y flip bits; DMG
backgrounds have no flip capability at all, GBC-only BG attribute) and
separately *reports but does not merge* 90/270-degree rotation matches
(no hardware rotate flag exists). See its module tests for the mechanics,
including the solid-color-collapse case.

## `cli/forge-pack` and `cli/forge-bank`

`forge-pack` turns a declared map (rows x cols of tile ids) or meta-sprite
(named tiles with x/y offsets + flip) into the index-only arrays real VRAM
tile-maps and OAM entries actually hold - never re-flattened pixel data.
Optionally bounds-checks declared tile ids against a `forge-png2tile
--out-map` JSON file via `parse_tile_map_ref`. Flip is encoded as the real
OAM attribute byte bits (0x20 = X flip, 0x40 = Y flip) - see
`flip_flags_byte` in `cli/forge-pack/src/lib.rs`.

`forge-bank` allocates a JSON manifest's items into 16KB MBC1/3/5 ROM banks:
items with an explicit `bank` pin (including `0`, the fixed/always-mapped
bank) are placed exactly there; everything else is "bankable" and packed
first-fit-decreasing by size into banks 1.. (bank 0 is never auto-filled).
Overflow - a pinned item not fitting its bank, or a bankable item not
fitting anywhere within the manifest's `cart_size` ceiling - is always a
hard error naming the item and the exact byte deficit, never a silent wrap.
Emits an RGBDS linker-script fragment (`SECTION "x", ROM0` for bank 0 items,
`SECTION "x", ROMX, BANK[n]` otherwise) alongside the JSON bank map.

Both are wired into the `ide/` shell's Pack and Bank panels
(`ide/src/pack_panel.rs`, `ide/src/bank_panel.rs`) calling their library
functions directly, matching the Png2Tile panel's no-subprocess pattern.

## Taking real screenshots of the `ide/` GUI headlessly

The sandbox this repo is usually worked in has no `Xvfb`/`xdotool`/window
manager preinstalled, no root/sudo, and no `apt`/full `dnf install` - but
`dnf download --resolve --destdir .` fetches RPMs (and their deps) without
root, and `rpm2cpio foo.rpm | cpio -idmv` extracts one into a local
directory whose binaries run fine against the system's own libs (same
Fedora release) with `LD_LIBRARY_PATH` pointed at the extracted `usr/lib64`.
This is how a from-scratch headless capture was done for the screenshots in
`docs/screenshots/`: `dnf download` + extract `xorg-x11-server-Xvfb`,
`xdotool` (+ its `libxdo` runtime dep), and `fluxbox` (a window manager is
**required** - `eframe`/`winit`'s GLX window never becomes visible to
`import -window root` without one managing/mapping it, even though the X
window itself is created and alive). Launch order: `Xvfb :99 -screen 0
1280x900x24`, then `fluxbox` on that display, then the app itself with
`env -u WAYLAND_DISPLAY DISPLAY=:99 ./target/release/forge-ide` (unsetting
an inherited `WAYLAND_DISPLAY` matters - winit's backend selection can be
thrown off by a stale Wayland socket env var even when targeting X11).
`import -window root` (ImageMagick, already present) captures the frame;
`xdotool mousemove/click`/`key`/`type` drives it. Several of the IDE's
panel buttons (`Toggle breakpoint`, `Watch`) sit in an `ui.horizontal` whose
preceding `text_edit_singleline` greedily claims the row width and pushes
the button past the dock panel's right edge, invisible and unclickable by
coordinate in the Breakpoints/Watch tabs at the default dock width -
`xdotool key Tab` then `Return`/`space` from the text field reaches it
regardless (keyboard focus order still includes it). Not an IDE bug worth
fixing for screenshot purposes, but worth knowing if driving those two
panels by coordinate click ever silently does nothing.

## The `ide/` crate (GUI shell)

`cargo run -p forge-ide` opens the actual IDE window: an `egui_dock`-based
multi-panel shell (Editor, Emulator, Registers, Memory, Watch, Disassembly,
Breakpoints, Console, PNG->Tile, Pack, Bank, each an independently
resizable/re-dockable tab - see `docs/getting-started.md` for the full tour)
over an embedded
TerminalGB emulator. The emulator panel depends on TerminalGB
(`github.com/Alchemy86/TerminalGB`) as a pinned git dependency, built with
`default-features = false` - the embedding build documented in that repo's
`docs/portability.md#7a-embedding-the-core`. That's a **private repo**, so
cloning/building here needs `gh auth switch --user Alchemy86` (or
equivalent credentials) and `.cargo/config.toml`'s `net.git-fetch-with-cli
= true` (Cargo's own git client doesn't reuse the system git credential
helper the way plain `git` does). `egui_dock` is pinned to `0.14`, the last
version matching this workspace's `egui 0.29` (check
`index.crates.io/eg/ui/egui_dock` before bumping either - the two version
lines are tied together, see their `Cargo.toml` deps).

The embedding build exposes `peek`/`peek_range`/`debug_read`/`debug_write`,
`debug_pc`/`debug_ime`/`debug_rom_bank`/`debug_halt_bug`,
`frame`/`step_instruction`, `image()`, and `save_state`/`load_state` -
**not** the full A/B/C/D/E/H/L/SP register file or a per-address
touched-list, and **no disassembler** (TerminalGB's `docs/debugging.md`
documents a planned `disassemble()` API but it isn't implemented at the
pinned rev). `ide/src/disasm.rs` is therefore a small from-scratch SM83
decoder written for the Disassembly panel - extend its opcode table rather
than re-checking TerminalGB for one first. The Watch panel
(`ide/src/emulator.rs`) diffs `peek_range` samples frame over frame to
infer writes rather than reading a real touched-address feed; a full
register view and a real touched-list are core-side additions, not IDE-side
gaps. `run_until_breakpoint` (`ide/src/emulator.rs`) is new IDE-side logic
on top of `step_instruction` - it has no core-side equivalent to call into.
`ide/assets/test-roms/gbselftest.gb` (MIT, mirrored from TerminalGB's
`third_party/gbselftest/`) is the bundled one-click smoke-test ROM.
`cargo run -p forge-ide --example headless_smoke` runs it headless and
asserts the framebuffer actually changes frame-to-frame; `cargo run -p
forge-ide --example run_rom -- <rom.gb> [frames] [out.png]` does the same
for any ROM file, with a per-frame PC/LY/LCDC trace - both the fastest way
to check the embedding (or a freshly built ROM) without a display.
`ide/src/emulator.rs`'s `#[cfg(test)]` module covers the breakpoint/step/
save-state logic directly against the bundled ROM, independent of any GUI
input delivery.

## `examples/hello-gb/`

The one real, buildable example in this repo: see its own `README.md` for
the exact `forge-png2tile` command and RGBDS build steps, and for a real
bug (an unset LCDC bit left the screen blank) that building it caught.
RGBDS isn't vendored here - fetch it the same way as `rgbgfx` above. Its
`.o`/`.gb` build output is gitignored; rebuild rather than expecting them
to be present.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
