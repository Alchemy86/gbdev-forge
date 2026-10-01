//! Invokes `forge-png2tile`'s library functions directly (no subprocess) so
//! the asset pipeline is reachable from inside the IDE, not only the CLI.

use std::path::PathBuf;

use forge_png2tile::{convert, dedup, flatten_rgba_over_white, load_png, summary_line};

#[derive(Default)]
pub struct Png2TilePanel {
    pub png_path: String,
    pub result: Option<Result<String, String>>,
}

impl Png2TilePanel {
    pub fn run(&mut self) {
        let path = PathBuf::from(self.png_path.trim());
        self.result = Some(Self::convert_one(&path));
    }

    fn convert_one(path: &std::path::Path) -> Result<String, String> {
        let dynamic = load_png(path).map_err(|e| e.to_string())?;
        let rgb = match dynamic {
            image::DynamicImage::ImageRgba8(rgba) => flatten_rgba_over_white(&rgba),
            other => other.to_rgb8(),
        };
        let converted = convert(&rgb, None).map_err(|e| e.to_string())?;
        let dedup_result = dedup(&converted.tiles);
        let summary = summary_line(converted.tiles.len(), &dedup_result);
        Ok(format!(
            "{}\nsource tiles: {}\nunique tiles: {}\nrotation-only matches: {}",
            summary,
            converted.tiles.len(),
            dedup_result.unique_tiles.len(),
            dedup_result.rotations.len(),
        ))
    }
}
