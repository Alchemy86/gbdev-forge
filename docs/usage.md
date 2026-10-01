# Usage guide

A step-by-step walkthrough of the `gbdev-forge` IDE, written against a real
run of the application -- every screenshot below is a real capture of the
actual `forge-ide` binary running (under Xvfb, with the embedded TerminalGB
core executing a real ROM), not a mockup. If a step here doesn't match what
you see, the software changed and this doc is stale -- fix the doc, not the
other way around. For the panel inventory and architecture notes, see
[`getting-started.md`](getting-started.md); this doc is the task-by-task
walkthrough.

## Prerequisites and build

```sh
cargo build --workspace
cargo test --workspace
```

Building `ide/` needs network access once (to fetch the pinned TerminalGB
git dependency) and, if you don't already have access to that private repo,
`gh auth switch --user Alchemy86` first -- see the root `AGENTS.md` for the
full detail. After that, everything is a normal local Cargo build.

## Launching the IDE

```sh
cargo run -p forge-ide
```

opens a single resizable window (`ide/src/main.rs`) with a toolbar across
the top and a dockable multi-panel layout below it: Editor and Emulator on
the left/center, Registers and a tabbed Memory/Watch/Breakpoints/Console
group on the right, and a tabbed Disassembly/PNG->Tile/Pack/Bank group
along the bottom.

## Loading and running a ROM

Click **Load bundled test ROM** in the toolbar -- this loads
`ide/assets/test-roms/gbselftest.gb` with no file picker needed, a
one-click smoke test. (**Load ROM** next to it loads whatever path is typed
into the ROM-path field instead, for any other `.gb` file.)

As soon as a ROM is loaded, Registers, Memory, and Disassembly start
showing real live data from the running core -- PC is `0100`, ROM bank `1`,
and the disassembly view shows the actual first instructions decoded from
the ROM:

![Main window with gbselftest ROM loaded and running -- registers, live WRAM, and disassembly all populated](screenshots/ide-emulator.png)

Toolbar controls, left to right:

- **Play / Pause** -- runs continuously at the emulator's own frame rate
  (toggles label depending on state).
- **Step instruction** -- executes exactly one CPU instruction.
- **Step frame** -- executes one full ~59.7 Hz frame.
- **Run to breakpoint** -- enabled once at least one breakpoint is set (see
  below).
- **Save state** / **Load state** -- an in-memory slot, not yet persisted to
  disk.

Click **Play**, let it run for a second, then **Pause** -- the Emulator
panel shows the actual framebuffer output. For `gbselftest.gb` specifically,
early frames show the test's text output (`CPU 15/15 ok`, `CYC 8/8 ok`,
etc. -- the screenshot above was captured at this point); letting it run
longer eventually reaches a blank test-complete screen, which is real ROM
behavior, not an IDE bug.

## Setting a breakpoint and watching execution pause

This is the one concrete walkthrough the rest of this section builds on.

1. Reload the ROM (**Load bundled test ROM** again) so PC resets to `0100`.
2. Switch to the **Breakpoints** tab (bottom-right group).
3. Type an address into the "Address (hex)" field and toggle it on. Note:
   the very first real instruction at `0100` is `NOP` immediately followed
   by an *unconditional* `JP $0200` at `0101` -- so a breakpoint anywhere in
   `0104..01FF` will never be hit from a fresh load, because that range is
   dead code on this ROM's actual control-flow path. `0200` **is** reached
   (it's the jump target), so that's what the walkthrough below uses. This
   is real, observed behavior of this specific ROM, not a limitation of the
   breakpoint feature -- see `ide/src/emulator.rs`'s own
   `run_until_breakpoint_stops_exactly_at_the_breakpoint` test for proof the
   mechanism itself stops exactly on a reachable address.
4. Click **Run to breakpoint** in the toolbar.

Execution runs until PC hits the breakpoint and stops -- exactly, not past
it:

![Breakpoint at $0200 hit -- PC reads 0200 exactly, Disassembly shows the LD ($C008),SP instruction at that address](screenshots/ide-breakpoints.png)

The `Console` tab logs breakpoint-set and breakpoint-hit events as a plain
execution log (not a REPL).

## Using the memory viewer and disassembly view

The **Memory** tab (right-side group) shows a live hex+ASCII dump of WRAM
(`$C000`-`$DFFF`), refreshed from the running core every frame. The
**Disassembly** panel (bottom-left group) decodes the next N instructions
from the current PC with a small from-scratch SM83 decoder
(`ide/src/disasm.rs`) -- the current instruction is marked `->`, any address
with a breakpoint set is marked `*`. Both update live as you Step or Play:

![Memory viewer (live WRAM hex dump) and Disassembly panel side by side, mid-execution](screenshots/ide-memory-disasm.png)

The **Instructions to show** slider on the Disassembly panel controls how
many decoded instructions are listed (4-64).

## Using the address watch

The **Watch** tab polls a chosen address range frame-over-frame and logs any
byte that changed -- a frame-diff, not a hardware write-trap (the embedded
core doesn't expose a touched-address feed; see `ide/src/emulator.rs`'s
module docs).

1. Switch to the **Watch** tab.
2. Type a start address (e.g. `C000`) and a length (e.g. `32`) and click
   **Watch**. (The button is off the right edge of the narrow default dock
   width in some layouts -- if you don't see it, widen the right dock panel
   first, or just Tab from the length field and press Enter/Space.)
3. Click **Play** for a moment, then **Pause**.

Each changed byte is logged with the frame it changed on and its old/new
value:

![Address watch on $C000..$C020 showing real frame-by-frame byte changes while the ROM runs](screenshots/ide-watch.png)

Click **Clear watch** to reset the log.

## Running `forge-png2tile` from inside the IDE

Switch to the **PNG->Tile** tab (bottom-left group). Type a path to a PNG
(or use **Browse...** for a file picker) and click **Convert** -- this calls
`forge-png2tile`'s library functions directly, no subprocess. Using this
repo's own `examples/hello-gb/smiley.png`:

![PNG->Tile panel after converting examples/hello-gb/smiley.png: 4 source tiles deduped to 2 unique tiles via flip-reuse](screenshots/ide-png2tile.png)

The result line reports source-tile count, unique-tile count after
flip-aware dedup, and rotation-equivalent-but-not-merged count (see the root
`AGENTS.md`'s "Smart tile dedup" section for why rotation matches are
reported but never auto-merged). The standalone CLI
(`cargo run -p forge-png2tile -- --help`) is the identical tool, usable
without the IDE.

## Running `forge-pack` from inside the IDE

Switch to the **Pack** tab. Two independent sub-tools live here:

- **Map**: type `rows,cols,tile ids...` (e.g. `2,2,0 1 1 0` for a 2x2 map
  reusing tile 0 and tile 1 twice each) and click **Pack map**.
- **Meta-sprite**: give it a name and one `tile_id offset_x offset_y flip`
  line per tile (flip: `none`/`h`/`v`/`hv`), then click **Pack meta-sprite**.

![Pack panel after packing both a 2x2 map and a two-tile meta-sprite -- the flip=0x20 result is the real OAM X-flip attribute bit](screenshots/ide-pack.png)

The meta-sprite result's `flip=0x20` is the literal OAM attribute byte bit
(`0x20` = X flip, `0x40` = Y flip) that `forge-pack` emits -- see
`flip_flags_byte` in `cli/forge-pack/src/lib.rs`.

## Try it all against `examples/hello-gb/`

`examples/hello-gb/` is the ready-made real example to practice all of the
above against: a hand-drawn 16x16 PNG (`smiley.png`), the exact
`forge-png2tile` command that generated its `.2bpp`/`.asm`/`.map.json`
output, and a minimal hand-written RGBDS program that displays it. See
`examples/hello-gb/README.md` for the exact commands and for a real bug its
own build caught (an unset LCDC bit left the screen blank) -- a good
reminder that this toolchain's output still has to be wired up correctly by
hand-written assembly, same as any other Game Boy dev workflow.
