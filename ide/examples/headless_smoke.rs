//! Headless smoke test for the embedded `terminalgb` core, independent of
//! the `egui` window -- proves the embedding actually executes a ROM and
//! produces real framebuffer output, the way TerminalGB's own `make embed`
//! example proves its embedding build (see
//! `docs/portability.md#7a-embedding-the-core` in that repo). Useful on its
//! own merits (a build-time smoke test for the embedding) and as a way to
//! check the core without a display server.
//!
//! `cargo run -p forge-ide --example headless_smoke -- [frames] [out.png]`

use terminalgb::gameboy::Gameboy;
use terminalgb::mode::Model;

const BUNDLED_ROM: &[u8] = include_bytes!("../assets/test-roms/gbselftest.gb");

fn frame_hash(rgba: &[u8]) -> u64 {
    // FNV-1a, good enough to show the buffer isn't static/blank frame to frame.
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in rgba {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn main() {
    let frames: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    let out_path = std::env::args().nth(2).unwrap_or_else(|| "/tmp/forge-ide-smoke.png".to_string());

    let mut gb = Gameboy::try_new_with_model(BUNDLED_ROM.to_vec(), None, Model::Auto)
        .expect("bundled ROM should load");
    println!("rom: {}", gb.rom_title());

    let mut last_hash = 0u64;
    let mut distinct_frames = 0u32;
    for i in 0..frames {
        gb.frame();
        let h = frame_hash(gb.image());
        if h != last_hash {
            distinct_frames += 1;
            last_hash = h;
        }
        if i % 30 == 0 {
            println!("frame {i:4}  pc={:04X}  hash={h:016x}", gb.debug_pc());
        }
    }
    println!("ran {frames} frames, {distinct_frames} distinct framebuffer states");
    assert!(
        distinct_frames > 1,
        "framebuffer never changed -- the core did not actually run"
    );

    let rgba = gb.image();
    image::save_buffer(&out_path, rgba, 160, 144, image::ColorType::Rgba8)
        .expect("failed to save screenshot");
    println!("wrote final frame to {out_path}");
}
