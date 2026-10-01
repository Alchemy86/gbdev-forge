mod disasm;
mod emulator;
mod png2tile_panel;

use std::path::PathBuf;

use eframe::egui;
use egui::{Color32, RichText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use terminalgb::KeypadKey;

use emulator::Emulator;
use png2tile_panel::Png2TilePanel;

/// A ROM bundled with the IDE itself so "load/run a ROM" has a one-click
/// smoke test with no external file needed -- gbselftest, MIT-licensed,
/// mirrored into `ide/assets/test-roms/` so the IDE is self-contained.
const BUNDLED_ROM: &[u8] = include_bytes!("../assets/test-roms/gbselftest.gb");

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
enum Tab {
    Editor,
    Emulator,
    Memory,
    Watch,
    Registers,
    Disassembly,
    Breakpoints,
    Console,
    Png2Tile,
}

impl Tab {
    fn title(&self) -> &'static str {
        match self {
            Tab::Editor => "Editor",
            Tab::Emulator => "Emulator",
            Tab::Memory => "Memory",
            Tab::Watch => "Watch",
            Tab::Registers => "Registers",
            Tab::Disassembly => "Disassembly",
            Tab::Breakpoints => "Breakpoints",
            Tab::Console => "Console",
            Tab::Png2Tile => "PNG->Tile",
        }
    }
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
    disasm_count: usize,

    // Watch panel inputs
    watch_addr_input: String,
    watch_len_input: String,

    // Breakpoint panel input
    breakpoint_input: String,

    // Save state (in-memory slot; "Save to file"/"Load from file" also
    // available via the core's own `save_state`/`load_state`, see
    // docs/mcp.md's `save_state`/`load_state` actions in TerminalGB).
    state_slot: Option<Vec<u8>>,

    png2tile: Png2TilePanel,
    dock_state: DockState<Tab>,
}

impl Default for ForgeIdeApp {
    fn default() -> Self {
        let mut dock_state = DockState::new(vec![Tab::Emulator]);
        let surface = dock_state.main_surface_mut();
        let [emulator_node, right] =
            surface.split_right(NodeIndex::root(), 0.7, vec![Tab::Registers]);
        let [_, _] = surface.split_below(
            right,
            0.4,
            vec![Tab::Memory, Tab::Watch, Tab::Breakpoints, Tab::Console],
        );
        let [_, _] = surface.split_left(emulator_node, 0.35, vec![Tab::Editor]);
        let [_, _] = surface.split_below(
            emulator_node,
            0.5,
            vec![Tab::Disassembly, Tab::Png2Tile],
        );

        Self {
            open_file: None,
            editor_text: String::new(),
            editor_status: String::new(),
            emulator: None,
            rom_path_input: String::new(),
            screen_texture: None,
            disasm_count: 12,
            watch_addr_input: "FF80".to_string(),
            watch_len_input: "16".to_string(),
            breakpoint_input: "0150".to_string(),
            state_slot: None,
            png2tile: Png2TilePanel::default(),
            dock_state,
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
                    breakpoints: Default::default(),
                    console: Vec::new(),
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

    fn toolbar(&mut self, ui: &mut egui::Ui) {
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
                let has_breakpoints = self
                    .emulator
                    .as_ref()
                    .map(|e| !e.breakpoints.is_empty())
                    .unwrap_or(false);
                ui.add_enabled_ui(has_breakpoints, |ui| {
                    if ui.button("Run to breakpoint").clicked() {
                        if let Some(emu) = &mut self.emulator {
                            emu.run_until_breakpoint();
                        }
                    }
                });
                ui.separator();
                if ui.button("Save state").clicked() {
                    if let Some(emu) = &mut self.emulator {
                        self.state_slot = Some(emu.save_state());
                        emu.log("state saved to in-memory slot");
                    }
                }
                ui.add_enabled_ui(self.state_slot.is_some(), |ui| {
                    if ui.button("Load state").clicked() {
                        if let (Some(emu), Some(data)) = (&mut self.emulator, &self.state_slot) {
                            match emu.load_state(data) {
                                Ok(()) => emu.log("state loaded from in-memory slot"),
                                Err(e) => emu.log(format!("load state failed: {e}")),
                            }
                        }
                    }
                });
            });
            if !self.editor_status.is_empty() {
                ui.separator();
                ui.label(&self.editor_status);
            }
        });
    }
}

/// Borrows the rest of `ForgeIdeApp`'s state to render each dock tab.
/// `egui_dock`'s `DockState` must be a separate field the app owns, so the
/// tab-rendering code is split into this short-lived viewer rather than a
/// method directly on `ForgeIdeApp` (the usual egui_dock split-borrow shape).
struct AppTabViewer<'a> {
    open_file: &'a Option<PathBuf>,
    editor_text: &'a mut String,
    emulator: &'a mut Option<Emulator>,
    screen_texture: &'a Option<egui::TextureHandle>,
    watch_addr_input: &'a mut String,
    watch_len_input: &'a mut String,
    breakpoint_input: &'a mut String,
    disasm_count: &'a mut usize,
    editor_status: &'a mut String,
    png2tile: &'a mut Png2TilePanel,
}

impl TabViewer for AppTabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Editor => self.editor_panel(ui),
            Tab::Emulator => self.emulator_panel(ui),
            Tab::Memory => self.memory_panel(ui),
            Tab::Watch => self.watch_panel(ui),
            Tab::Registers => self.registers_panel(ui),
            Tab::Disassembly => self.disassembly_panel(ui),
            Tab::Breakpoints => self.breakpoints_panel(ui),
            Tab::Console => self.console_panel(ui),
            Tab::Png2Tile => self.png2tile_panel(ui),
        }
    }
}

impl AppTabViewer<'_> {
    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(
            self.open_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(no file open)".to_string()),
        );
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(self.editor_text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(40),
            );
        });
    }

    fn emulator_panel(&mut self, ui: &mut egui::Ui) {
        if let Some(tex) = self.screen_texture {
            let size = tex.size_vec2() * 3.0;
            ui.image((tex.id(), size));
        } else {
            ui.label("No ROM loaded. Use \"Load bundled test ROM\" for a quick smoke test.");
        }
        if let Some(emu) = self.emulator.as_ref() {
            ui.label(format!("{} -- {}", emu.rom_title, emu.rom_path));
            ui.label(
                RichText::new("Arrow keys / Z (A) / X (B) / Enter (Start) / Backspace (Select)")
                    .weak()
                    .small(),
            );
        }
    }

    fn memory_panel(&mut self, ui: &mut egui::Ui) {
        let Some(emu) = self.emulator.as_mut() else {
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
        let Some(emu) = self.emulator.as_ref() else {
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
            ui.text_edit_singleline(self.watch_addr_input);
            ui.label("Length:");
            ui.text_edit_singleline(self.watch_len_input);
            if ui.button("Watch").clicked() {
                let addr = u16::from_str_radix(self.watch_addr_input.trim_start_matches("0x"), 16);
                let len: Result<usize, _> = self.watch_len_input.trim().parse();
                if let (Ok(addr), Ok(len)) = (addr, len) {
                    if let Some(emu) = self.emulator.as_mut() {
                        emu.start_watch(addr, len.max(1));
                    }
                } else {
                    *self.editor_status = "watch: bad address or length".to_string();
                }
            }
            if ui.button("Clear watch").clicked() {
                if let Some(emu) = self.emulator.as_mut() {
                    emu.clear_watch();
                }
            }
        });
        ui.separator();
        let Some(emu) = self.emulator.as_ref() else {
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

    fn disassembly_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Instructions to show:");
            ui.add(egui::Slider::new(self.disasm_count, 4..=64));
        });
        ui.separator();
        let Some(emu) = self.emulator.as_mut() else {
            ui.label("Load a ROM to disassemble around PC.");
            return;
        };
        let pc = emu.gb.debug_pc();
        let count = *self.disasm_count;
        let breakpoints = emu.breakpoints.clone();
        let gb = &mut emu.gb;
        let instrs = disasm::disassemble(|a| gb.debug_read(a), pc, count);
        egui::ScrollArea::vertical()
            .max_height(ui.available_height())
            .show(ui, |ui| {
                for instr in &instrs {
                    let is_pc = instr.address == pc;
                    let is_bp = breakpoints.contains(&instr.address);
                    let bytes_hex: String = instr
                        .bytes
                        .iter()
                        .map(|b| format!("{b:02X} "))
                        .collect();
                    let marker = if is_pc { "-> " } else if is_bp { " * " } else { "   " };
                    let line = format!("{marker}{:04X}  {:<9}{}", instr.address, bytes_hex, instr.text);
                    let text = RichText::new(line).monospace();
                    let text = if is_pc {
                        text.color(Color32::from_rgb(120, 200, 120))
                    } else if is_bp {
                        text.color(Color32::from_rgb(220, 140, 80))
                    } else {
                        text
                    };
                    ui.label(text);
                }
            });
        ui.label(
            RichText::new(
                "New decoder: TerminalGB documents a disassemble() API in docs/debugging.md \
                 but doesn't implement it in the pinned embedding build, so this view's SM83 \
                 decode table lives in ide/src/disasm.rs.",
            )
            .weak()
            .small(),
        );
    }

    fn breakpoints_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Address (hex):");
            ui.text_edit_singleline(self.breakpoint_input);
            if ui.button("Toggle breakpoint").clicked() {
                if let Ok(addr) = u16::from_str_radix(self.breakpoint_input.trim_start_matches("0x"), 16) {
                    if let Some(emu) = self.emulator.as_mut() {
                        emu.toggle_breakpoint(addr);
                    }
                } else {
                    *self.editor_status = "breakpoint: bad address".to_string();
                }
            }
        });
        ui.separator();
        let Some(emu) = self.emulator.as_mut() else {
            ui.label("Load a ROM to set breakpoints.");
            return;
        };
        if emu.breakpoints.is_empty() {
            ui.label("No breakpoints set.");
            return;
        }
        let mut to_remove = None;
        for &addr in &emu.breakpoints {
            ui.horizontal(|ui| {
                ui.monospace(format!("{addr:04X}"));
                if ui.small_button("remove").clicked() {
                    to_remove = Some(addr);
                }
            });
        }
        if let Some(addr) = to_remove {
            emu.toggle_breakpoint(addr);
        }
    }

    fn console_panel(&mut self, ui: &mut egui::Ui) {
        let Some(emu) = self.emulator.as_mut() else {
            ui.label("Load a ROM to see the execution log.");
            return;
        };
        if ui.button("Clear log").clicked() {
            emu.console.clear();
        }
        egui::ScrollArea::vertical()
            .max_height(ui.available_height())
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if emu.console.is_empty() {
                    ui.label("(no events logged yet -- breakpoint hits and state loads show up here)");
                }
                for line in &emu.console {
                    ui.monospace(line);
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
            self.toolbar(ui);
        });

        let mut viewer = AppTabViewer {
            open_file: &self.open_file,
            editor_text: &mut self.editor_text,
            emulator: &mut self.emulator,
            screen_texture: &self.screen_texture,
            watch_addr_input: &mut self.watch_addr_input,
            watch_len_input: &mut self.watch_len_input,
            breakpoint_input: &mut self.breakpoint_input,
            disasm_count: &mut self.disasm_count,
            editor_status: &mut self.editor_status,
            png2tile: &mut self.png2tile,
        };
        DockArea::new(&mut self.dock_state)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut viewer);
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
