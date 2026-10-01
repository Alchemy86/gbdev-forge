//! Headless runner for an arbitrary ROM file, the same embedding path
//! `headless_smoke` exercises for the bundled test ROM -- useful to sanity
//! check any ROM (including a freshly built one, e.g. `examples/hello-gb`)
//! without opening the `egui` window.
//!
//! `cargo run -p forge-ide --example run_rom -- <rom.gb> [frames] [out.png]`

use terminalgb::gameboy::Gameboy;
use terminalgb::mode::Model;

fn main() {
    let mut args = std::env::args().skip(1);
    let rom_path = args.next().expect("usage: run_rom <rom.gb> [frames] [out.png]");
    let frames: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let out_path = args.next().unwrap_or_else(|| "/tmp/run_rom.png".to_string());

    let data = std::fs::read(&rom_path).expect("failed to read ROM file");
    let mut gb = Gameboy::try_new_with_model(data, None, Model::Auto).expect("ROM should load");
    println!("rom: {}", gb.rom_title());

    for i in 0..frames {
        gb.frame();
        println!("frame {i:4}  pc={:04X}  ly={:02X}  lcdc={:02X}", gb.debug_pc(), gb.peek(0xFF44), gb.peek(0xFF40));
    }

    let rgba = gb.image();
    image::save_buffer(&out_path, rgba, 160, 144, image::ColorType::Rgba8)
        .expect("failed to save screenshot");
    println!("wrote final frame to {out_path}");
}
