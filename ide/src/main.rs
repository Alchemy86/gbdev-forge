mod emulator;
mod png2tile_panel;

use std::path::PathBuf;

use eframe::egui;
use egui::{Color32, RichText};
use terminalgb::KeypadKey;

use emulator::Emulator;
use png2tile_panel::Png2TilePanel;

/// A ROM bundled with the IDE itself so "load/run a ROM" has a one-click
/// smoke test with no external file needed -- gbselftest, MIT-licensed,
/// mirrored into `ide/assets/test-roms/` so the IDE is self-contained.
const BUNDLED_ROM: &[u8] = include_bytes!("../assets/test-roms/gbselftest.gb");

#[derive(PartialEq, Eq, Clone, Copy)]
enum RightTab {
    Memory,
    Watch,
    Registers,
    Png2Tile,
}

struct ForgeIdeApp {
    // Code editor
    open_file: Option<PathBuf>,
    editor_text: String,
    editor_status: String,

    // Emulator
    emulator: Option<Emulator>,
    rom_path_input: String,
    screen_texture: Option<egui::TextureHandle>,

    // Watch panel inputs
    watch_addr_input: String,
    watch_len_input: String,

    right_tab: RightTab,
    png2tile: Png2TilePanel,
}

impl Default for ForgeIdeApp {
    fn default() -> Self {
        Self {
            open_file: None,
            editor_text: String::new(),
            editor_status: String::new(),
            emulator: None,
            rom_path_input: String::new(),
            screen_texture: None,
            watch_addr_input: "FF80".to_string(),
            watch_len_input: "16".to_string(),
            right_tab: RightTab::Memory,
            png2tile: Png2TilePanel::default(),
        }
    }
}

impl ForgeIdeApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    fn open_file_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    self.editor_text = text;
                    self.open_file = Some(path);
                    self.editor_status = "opened".to_string();
                }
                Err(e) => self.editor_status = format!("failed to open: {e}"),
            }
        }
    }

    fn save_file(&mut self) {
        let Some(path) = self.open_file.clone() else {
            if let Some(path) = rfd::FileDialog::new().save_file() {
                self.open_file = Some(path);
            } else {
                return;
            }
            return self.save_file();
        };
        match std::fs::write(&path, &self.editor_text) {
            Ok(()) => self.editor_status = format!("saved {}", path.display()),
            Err(e) => self.editor_status = format!("save failed: {e}"),
        }
    }

    fn load_rom(&mut self, path: PathBuf) {
        match Emulator::load(&path) {
            Ok(emu) => {
                self.editor_status.clear();
                self.emulator = Some(emu);
            }
            Err(e) => self.editor_status = format!("ROM load failed: {e}"),
        }
    }

    fn load_bundled_rom(&mut self) {
        let data = BUNDLED_ROM.to_vec();
        match terminalgb::gameboy::Gameboy::try_new_with_model(
            data,
            None,
            terminalgb::mode::Model::Auto,
        ) {
            Ok(gb) => {
                let rom_title = gb.rom_title();
                self.emulator = Some(Emulator {
                    rom_path: "<bundled gbselftest.gb>".to_string(),
                    rom_title,
                    running: false,
                    frame_count: 0,
                    watch: None,
                    gb,
                });
            }
            Err(e) => self.editor_status = format!("bundled ROM load failed: {e}"),
        }
    }

    fn update_screen_texture(&mut self, ctx: &egui::Context) {
        let Some(emu) = &self.emulator else { return };
        let rgba = emu.gb.image();
        let image = egui::ColorImage::from_rgba_unmultiplied([160, 144], rgba);
        match &mut self.screen_texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.screen_texture = Some(ctx.load_texture(
                    "gb-screen",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let Some(emu) = &mut self.emulator else { return };
        if !emu.running {
            return;
        }
        ctx.input(|i| {
            const MAP: &[(egui::Key, KeypadKey)] = &[
                (egui::Key::ArrowUp, KeypadKey::Up),
                (egui::Key::ArrowDown, KeypadKey::Down),
                (egui::Key::ArrowLeft, KeypadKey::Left),
                (egui::Key::ArrowRight, KeypadKey::Right),
                (egui::Key::Z, KeypadKey::A),
                (egui::Key::X, KeypadKey::B),
                (egui::Key::Enter, KeypadKey::Start),
                (egui::Key::Backspace, KeypadKey::Select),
            ];
            for (egui_key, gb_key) in MAP {
                if i.key_pressed(*egui_key) {
                    emu.key_down(*gb_key);
                }
                if i.key_released(*egui_key) {
                    emu.key_up(*gb_key);
                }
            }
        });
    }

    fn memory_panel(&mut self, ui: &mut egui::Ui) {
        let Some(emu) = &mut self.emulator else {
            ui.label("Load a ROM to see memory.");
            return;
        };
        ui.label(RichText::new("WRAM 0xC000-0xDFFF (live)").strong());
        let dump = emu.wram_dump();
        egui::ScrollArea::vertical()
            .max_height(ui.available_height())
            .show(ui, |ui| {
                let mut text = String::with_capacity(dump.len() * 4);
                for (row, chunk) in dump.chunks(16).enumerate() {
                    let addr = emulator::WRAM_START as usize + row * 16;
                    text.push_str(&format!("{addr:04X}: "));
                    for b in chunk {
                        text.push_str(&format!("{b:02X} "));
                    }
                    text.push_str(" |");
                    for &b in chunk {
                        let c = if (0x20..0x7f).contains(&b) { b as char } else { '.' };
                        text.push(c);
                    }
                    text.push_str("|\n");
                }
                ui.monospace(text);
            });
    }

    fn registers_panel(&mut self, ui: &mut egui::Ui) {
        let Some(emu) = &self.emulator else {
            ui.label("Load a ROM to see registers.");
            return;
        };
        ui.label(RichText::new(&emu.rom_title).strong());
        ui.separator();
        egui::Grid::new("registers_grid").num_columns(2).show(ui, |ui| {
            ui.label("PC");
            ui.monospace(format!("{:04X}", emu.gb.debug_pc()));
            ui.end_row();
            ui.label("IME");
            ui.monospace(format!("{}", emu.gb.debug_ime()));
            ui.end_row();
            ui.label("ROM bank");
            ui.monospace(format!("{}", emu.gb.debug_rom_bank()));
            ui.end_row();
            ui.label("HALT bug latch");
            ui.monospace(format!("{}", emu.gb.debug_halt_bug()));
            ui.end_row();
            ui.label("Frame");
            ui.monospace(format!("{}", emu.frame_count));
            ui.end_row();
            ui.label("Host writes");
            ui.monospace(format!("{}", emu.gb.host_writes()));
            ui.end_row();
        });
        ui.separator();
        ui.label(
            RichText::new(
                "Full A/B/C/D/E/H/L/SP/flag registers aren't on the core's public \
                 embedding surface yet (only PC/IME/ROM-bank/HALT-bug are) -- a \
                 follow-up core change, not an IDE limitation.",
            )
            .weak()
            .small(),
        );
    }

    fn watch_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Address (hex):");
            ui.text_edit_singleline(&mut self.watch_addr_input);
            ui.label("Length:");
            ui.text_edit_singleline(&mut self.watch_len_input);
            if ui.button("Watch").clicked() {
                let addr = u16::from_str_radix(self.watch_addr_input.trim_start_matches("0x"), 16);
                let len: Result<usize, _> = self.watch_len_input.trim().parse();
                if let (Ok(addr), Ok(len)) = (addr, len) {
                    if let Some(emu) = &mut self.emulator {
                        emu.start_watch(addr, len.max(1));
                    }
                } else {
                    self.editor_status = "watch: bad address or length".to_string();
                }
            }
            if ui.button("Clear watch").clicked() {
                if let Some(emu) = &mut self.emulator {
                    emu.clear_watch();
                }
            }
        });
        ui.separator();
        let Some(emu) = &self.emulator else {
            ui.label("Load a ROM, then watch an address range.");
            return;
        };
        let Some(watch) = &emu.watch else {
            ui.label("No active watch.");
            return;
        };
        ui.label(format!(
            "Watching {:04X}..{:04X} ({} bytes) -- {} write(s) observed this session",
            watch.start,
            watch.start as usize + watch.len,
            watch.len,
            watch.total_writes(),
        ));
        egui::ScrollArea::vertical()
            .max_height(ui.available_height())
            .show(ui, |ui| {
                for ev in watch.log.iter().rev().take(200) {
                    ui.monospace(format!(
                        "frame {:>8}  {:04X}: {:02X} -> {:02X}",
                        ev.frame, ev.address, ev.old, ev.new
                    ));
                }
            });
    }

    fn png2tile_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("forge-png2tile (library call, no subprocess)").strong());
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.png2tile.png_path);
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .pick_file()
                {
                    self.png2tile.png_path = path.display().to_string();
                }
            }
            if ui.button("Convert").clicked() {
                self.png2tile.run();
            }
        });
        ui.separator();
        match &self.png2tile.result {
            Some(Ok(text)) => {
                ui.colored_label(Color32::from_rgb(120, 200, 120), text);
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::from_rgb(220, 100, 100), e);
            }
            None => {
                ui.label("No conversion run yet.");
            }
        }
    }
}

impl eframe::App for ForgeIdeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_input(ctx);

        if let Some(emu) = &mut self.emulator {
            if emu.running {
                emu.step_frame();
                ctx.request_repaint();
            }
        }
        self.update_screen_texture(ctx);

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open file").clicked() {
                    self.open_file_dialog();
                }
                if ui.button("Save file").clicked() {
                    self.save_file();
                }
                ui.separator();
                ui.label("ROM path:");
                ui.text_edit_singleline(&mut self.rom_path_input);
                if ui.button("Load ROM").clicked() {
                    let path = PathBuf::from(self.rom_path_input.trim());
                    self.load_rom(path);
                }
                if ui.button("Load bundled test ROM").clicked() {
                    self.load_bundled_rom();
                }
                ui.separator();
                let has_emu = self.emulator.is_some();
                ui.add_enabled_ui(has_emu, |ui| {
                    let running = self.emulator.as_ref().map(|e| e.running).unwrap_or(false);
                    if ui.button(if running { "Pause" } else { "Play" }).clicked() {
                        if let Some(emu) = &mut self.emulator {
                            emu.running = !emu.running;
                        }
                    }
                    if ui.button("Step instruction").clicked() {
                        if let Some(emu) = &mut self.emulator {
                            emu.step_instruction();
                        }
                    }
                    if ui.button("Step frame").clicked() {
                        if let Some(emu) = &mut self.emulator {
                            emu.step_frame();
                        }
                    }
                });
                if !self.editor_status.is_empty() {
                    ui.separator();
                    ui.label(&self.editor_status);
                }
            });
        });

        egui::SidePanel::left("editor_panel")
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(
                    self.open_file
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(no file open)".to_string()),
                );
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.editor_text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(40),
                    );
                });
            });

        egui::SidePanel::right("debug_panel")
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.right_tab, RightTab::Memory, "Memory");
                    ui.selectable_value(&mut self.right_tab, RightTab::Watch, "Watch");
                    ui.selectable_value(&mut self.right_tab, RightTab::Registers, "Registers");
                    ui.selectable_value(&mut self.right_tab, RightTab::Png2Tile, "PNG->Tile");
                });
                ui.separator();
                match self.right_tab {
                    RightTab::Memory => self.memory_panel(ui),
                    RightTab::Watch => self.watch_panel(ui),
                    RightTab::Registers => self.registers_panel(ui),
                    RightTab::Png2Tile => self.png2tile_panel(ui),
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(RichText::new("Emulator").strong());
            if let Some(tex) = &self.screen_texture {
                let size = tex.size_vec2() * 3.0;
                ui.image((tex.id(), size));
            } else {
                ui.label("No ROM loaded. Use \"Load bundled test ROM\" for a quick smoke test.");
            }
            if let Some(emu) = &self.emulator {
                ui.label(format!("{} -- {}", emu.rom_title, emu.rom_path));
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "gbdev-forge IDE",
        native_options,
        Box::new(|cc| Ok(Box::new(ForgeIdeApp::new(cc)))),
    )
}
