//! Invokes `forge-pack`'s library functions directly (no subprocess) so
//! map/meta-sprite packing is reachable from inside the IDE, not only the CLI.

use forge_pack::{pack_map, pack_meta_sprite, MapDecl, MetaSpriteDecl, MetaSpriteTileDecl};

#[derive(Default)]
pub struct PackPanel {
    /// "rows,cols,tile_id tile_id tile_id..." (row-major, whitespace-separated).
    pub map_input: String,
    pub map_result: Option<Result<String, String>>,

    pub sprite_name: String,
    /// One "tile_id offset_x offset_y flip" per line.
    pub sprite_input: String,
    pub sprite_result: Option<Result<String, String>>,
}

impl PackPanel {
    pub fn run_map(&mut self) {
        self.map_result = Some(Self::pack_map_from_input(&self.map_input));
    }

    fn pack_map_from_input(input: &str) -> Result<String, String> {
        let mut parts = input.splitn(2, ',');
        let rows: usize = parts
            .next()
            .ok_or("expected \"rows,cols,tiles...\"")?
            .trim()
            .parse()
            .map_err(|_| "rows is not a number".to_string())?;
        let rest = parts.next().ok_or("expected \"rows,cols,tiles...\"")?;
        let mut rest_parts = rest.splitn(2, ',');
        let cols: usize = rest_parts
            .next()
            .ok_or("expected \"rows,cols,tiles...\"")?
            .trim()
            .parse()
            .map_err(|_| "cols is not a number".to_string())?;
        let tiles_str = rest_parts.next().unwrap_or("");
        let tiles: Result<Vec<usize>, _> = tiles_str
            .split_whitespace()
            .map(|s| s.parse::<usize>())
            .collect();
        let tiles = tiles.map_err(|_| "tile ids must be whole numbers".to_string())?;

        let decl = MapDecl { rows, cols, tiles };
        let output = pack_map(&decl, None).map_err(|e| e.to_string())?;
        Ok(format!(
            "{}x{} map -> tile_ids: {:?}",
            output.rows, output.cols, output.tile_ids
        ))
    }

    pub fn run_sprite(&mut self) {
        self.sprite_result = Some(Self::pack_sprite_from_input(
            &self.sprite_name,
            &self.sprite_input,
        ));
    }

    fn pack_sprite_from_input(name: &str, input: &str) -> Result<String, String> {
        let mut tiles = Vec::new();
        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 4 {
                return Err(format!(
                    "line \"{line}\" must have 4 fields: tile_id offset_x offset_y flip"
                ));
            }
            let tile_id: usize = fields[0].parse().map_err(|_| "bad tile_id".to_string())?;
            let offset_x: i16 = fields[1].parse().map_err(|_| "bad offset_x".to_string())?;
            let offset_y: i16 = fields[2].parse().map_err(|_| "bad offset_y".to_string())?;
            tiles.push(MetaSpriteTileDecl {
                tile_id,
                offset_x,
                offset_y,
                flip: fields[3].to_string(),
            });
        }
        let decl = MetaSpriteDecl {
            name: name.to_string(),
            tiles,
        };
        let output = pack_meta_sprite(&decl, None).map_err(|e| e.to_string())?;
        let mut lines: Vec<String> = Vec::with_capacity(output.tiles.len());
        for t in &output.tiles {
            lines.push(format!(
                "tile {} @ ({}, {}) flip=0x{:02X}",
                t.tile_id, t.offset_x, t.offset_y, t.flip_flags
            ));
        }
        Ok(format!("\"{}\": {} tile(s)\n{}", output.name, output.tiles.len(), lines.join("\n")))
    }
}
