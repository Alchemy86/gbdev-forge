//! Conversion logic for `forge-png2tile`, split out from `main.rs` so it is
//! directly testable without spawning the binary.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use forge_core::{dedup_tiles, DedupResult, Palette, Rgb, Tile};
use image::{Rgb as ImgRgb, RgbImage, RgbaImage};
use serde::Serialize;

/// Euclidean RGB distance below which two colors are treated as the same
/// shade (anti-aliasing noise / minor compression artifacts), and above
/// which an explicit palette color is considered "not a match".
pub const COLOR_TOLERANCE: f32 = 24.0;

#[derive(Debug)]
pub enum ConvertError {
    DimensionsNotMultipleOf8 { width: u32, height: u32 },
    AlphaWithoutFlatten,
    TooManyColors { count: usize },
    ColorNotInPalette { color: Rgb },
    PaletteFile(String),
    Image(String),
    Io(String),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::DimensionsNotMultipleOf8 { width, height } => write!(
                f,
                "image dimensions {width}x{height} are not both multiples of 8"
            ),
            ConvertError::AlphaWithoutFlatten => write!(
                f,
                "input PNG has an alpha channel; pass --flatten-alpha to composite it over a white background before converting"
            ),
            ConvertError::TooManyColors { count } => write!(
                f,
                "image resolves to {count} distinct colors after quantization tolerance, but Game Boy 2BPP tiles support at most 4; supply --palette or reduce the source image's colors"
            ),
            ConvertError::ColorNotInPalette { color } => write!(
                f,
                "color rgb({}, {}, {}) does not match any of the 4 supplied palette colors within tolerance",
                color.r, color.g, color.b
            ),
            ConvertError::PaletteFile(msg) => write!(f, "invalid palette file: {msg}"),
            ConvertError::Image(msg) => write!(f, "failed to read PNG: {msg}"),
            ConvertError::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<io::Error> for ConvertError {
    fn from(e: io::Error) -> Self {
        ConvertError::Io(e.to_string())
    }
}

#[derive(Debug)]
pub struct ConvertedImage {
    /// Source tiles, in left-to-right top-to-bottom reading order.
    pub tiles: Vec<Tile>,
    pub palette: Palette,
}

/// Parse a palette file: exactly 4 non-empty lines, each "R G B" (decimal,
/// whitespace- or comma-separated).
pub fn parse_palette_file(path: &Path) -> Result<Palette, ConvertError> {
    let contents = fs::read_to_string(path)
        .map_err(|e| ConvertError::PaletteFile(format!("{}: {e}", path.display())))?;
    let lines: Vec<&str> = contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() != 4 {
        return Err(ConvertError::PaletteFile(format!(
            "expected exactly 4 color lines, found {}",
            lines.len()
        )));
    }
    let mut colors = [Rgb::new(0, 0, 0); 4];
    for (i, line) in lines.iter().enumerate() {
        let parts: Vec<&str> = line
            .split([',', ' ', '\t'])
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() != 3 {
            return Err(ConvertError::PaletteFile(format!(
                "line {}: expected \"R G B\", got \"{line}\"",
                i + 1
            )));
        }
        let mut channel = [0u8; 3];
        for (c, part) in channel.iter_mut().zip(parts.iter()) {
            *c = part.parse::<u8>().map_err(|_| {
                ConvertError::PaletteFile(format!("line {}: \"{part}\" is not 0-255", i + 1))
            })?;
        }
        colors[i] = Rgb::new(channel[0], channel[1], channel[2]);
    }
    Ok(Palette { colors })
}

/// Composite an RGBA image over a solid white background, producing an
/// opaque RGB image.
pub fn flatten_rgba_over_white(img: &RgbaImage) -> RgbImage {
    let mut out = RgbImage::new(img.width(), img.height());
    for (x, y, px) in img.enumerate_pixels() {
        let [r, g, b, a] = px.0;
        let a = a as u32;
        let blend =
            |fg: u8, bg: u8| -> u8 { ((fg as u32 * a + bg as u32 * (255 - a)) / 255) as u8 };
        out.put_pixel(x, y, ImgRgb([blend(r, 255), blend(g, 255), blend(b, 255)]));
    }
    out
}

fn key(px: &ImgRgb<u8>) -> [u8; 3] {
    px.0
}

/// Quantize an RGB image to <=4 palette indices and slice it into 8x8
/// tiles in left-to-right, top-to-bottom reading order.
pub fn convert(img: &RgbImage, palette: Option<Palette>) -> Result<ConvertedImage, ConvertError> {
    let (width, height) = img.dimensions();
    if width % 8 != 0 || height % 8 != 0 {
        return Err(ConvertError::DimensionsNotMultipleOf8 { width, height });
    }

    // Distinct colors in first-seen (row-major) order.
    let mut seen = std::collections::HashSet::new();
    let mut distinct: Vec<[u8; 3]> = Vec::new();
    for (_, _, px) in img.enumerate_pixels() {
        let k = key(px);
        if seen.insert(k) {
            distinct.push(k);
        }
    }

    let (color_to_index, resolved_palette) = match palette {
        Some(given) => {
            let mut color_to_index = HashMap::new();
            for color in &distinct {
                let rgb = Rgb::new(color[0], color[1], color[2]);
                let mut best_idx = 0usize;
                let mut best_dist = f32::MAX;
                for (i, pc) in given.colors.iter().enumerate() {
                    let d = rgb.distance(pc);
                    if d < best_dist {
                        best_dist = d;
                        best_idx = i;
                    }
                }
                if best_dist > COLOR_TOLERANCE {
                    return Err(ConvertError::ColorNotInPalette { color: rgb });
                }
                color_to_index.insert(*color, best_idx as u8);
            }
            (color_to_index, given)
        }
        None => {
            // Greedy tolerance clustering, first-seen representative per
            // cluster, then index-ordered brightest (0) to darkest (3).
            let mut cluster_reps: Vec<[u8; 3]> = Vec::new();
            let mut color_to_cluster: HashMap<[u8; 3], usize> = HashMap::new();
            for color in &distinct {
                let rgb = Rgb::new(color[0], color[1], color[2]);
                let mut matched = None;
                for (i, rep) in cluster_reps.iter().enumerate() {
                    let rep_rgb = Rgb::new(rep[0], rep[1], rep[2]);
                    if rgb.distance(&rep_rgb) <= COLOR_TOLERANCE {
                        matched = Some(i);
                        break;
                    }
                }
                let cluster_idx = match matched {
                    Some(i) => i,
                    None => {
                        cluster_reps.push(*color);
                        cluster_reps.len() - 1
                    }
                };
                color_to_cluster.insert(*color, cluster_idx);
            }
            if cluster_reps.len() > 4 {
                return Err(ConvertError::TooManyColors {
                    count: cluster_reps.len(),
                });
            }

            let mut order: Vec<usize> = (0..cluster_reps.len()).collect();
            order.sort_by(|&a, &b| {
                let la = Rgb::new(cluster_reps[a][0], cluster_reps[a][1], cluster_reps[a][2])
                    .luminance();
                let lb = Rgb::new(cluster_reps[b][0], cluster_reps[b][1], cluster_reps[b][2])
                    .luminance();
                lb.total_cmp(&la) // brightest first -> index 0
            });
            let mut final_index_of_cluster = vec![0u8; cluster_reps.len()];
            for (final_idx, &orig_idx) in order.iter().enumerate() {
                final_index_of_cluster[orig_idx] = final_idx as u8;
            }

            let mut colors = [Rgb::new(0, 0, 0); 4];
            for (final_idx, &orig_idx) in order.iter().enumerate() {
                let c = cluster_reps[orig_idx];
                colors[final_idx] = Rgb::new(c[0], c[1], c[2]);
            }
            // Pad unused slots (fewer than 4 real shades) with black so the
            // Palette type always carries 4 entries.
            for slot in colors.iter_mut().skip(order.len()) {
                *slot = Rgb::new(0, 0, 0);
            }

            let mut color_to_index = HashMap::new();
            for (color, &cluster_idx) in &color_to_cluster {
                color_to_index.insert(*color, final_index_of_cluster[cluster_idx]);
            }
            (color_to_index, Palette { colors })
        }
    };

    let tiles_x = width / 8;
    let tiles_y = height / 8;
    let mut tiles = Vec::with_capacity((tiles_x * tiles_y) as usize);
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let mut pixels = [[0u8; 8]; 8];
            for row in 0..8u32 {
                for col in 0..8u32 {
                    let px = img.get_pixel(tx * 8 + col, ty * 8 + row);
                    let idx = color_to_index[&key(px)];
                    pixels[row as usize][col as usize] = idx;
                }
            }
            tiles.push(Tile { pixels });
        }
    }

    Ok(ConvertedImage {
        tiles,
        palette: resolved_palette,
    })
}

pub fn write_2bpp(path: &Path, tiles: &[Tile]) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(tiles.len() * 16);
    for t in tiles {
        bytes.extend_from_slice(&t.to_2bpp_bytes());
    }
    fs::write(path, bytes)
}

pub fn write_c_array(path: &Path, name: &str, tiles: &[Tile]) -> io::Result<()> {
    let mut out = String::new();
    out.push_str(&format!("const uint8_t {name}[] = {{\n"));
    for (i, t) in tiles.iter().enumerate() {
        let bytes = t.to_2bpp_bytes();
        let hex: Vec<String> = bytes.iter().map(|b| format!("0x{b:02X}")).collect();
        out.push_str(&format!("    {}, // tile {i}\n", hex.join(", ")));
    }
    out.push_str("};\n");
    fs::write(path, out)
}

pub fn write_asm_array(path: &Path, name: &str, tiles: &[Tile]) -> io::Result<()> {
    let mut out = String::new();
    out.push_str(&format!("{name}:\n"));
    for (i, t) in tiles.iter().enumerate() {
        let bytes = t.to_2bpp_bytes();
        let hex: Vec<String> = bytes.iter().map(|b| format!("${b:02X}")).collect();
        out.push_str(&format!("    DB {} ; tile {i}\n", hex.join(",")));
    }
    fs::write(path, out)
}

#[derive(Serialize)]
struct TileRefOut {
    source_index: usize,
    unique_id: usize,
    flip: &'static str,
}

#[derive(Serialize)]
struct RotationOut {
    source_index: usize,
    matches_unique_id: usize,
    rotation_degrees: u16,
}

#[derive(Serialize)]
struct TileMapOut {
    source_tile_count: usize,
    unique_tile_count: usize,
    references: Vec<TileRefOut>,
    rotation_findings: Vec<RotationOut>,
}

pub fn write_tile_map_json(path: &Path, dedup: &DedupResult) -> io::Result<()> {
    let out = TileMapOut {
        source_tile_count: dedup.references.len(),
        unique_tile_count: dedup.unique_tiles.len(),
        references: dedup
            .references
            .iter()
            .enumerate()
            .map(|(source_index, r)| TileRefOut {
                source_index,
                unique_id: r.unique_id,
                flip: r.flip.as_str(),
            })
            .collect(),
        rotation_findings: dedup
            .rotations
            .iter()
            .map(|r| RotationOut {
                source_index: r.source_index,
                matches_unique_id: r.matches_unique_id,
                rotation_degrees: r.rotation.degrees(),
            })
            .collect(),
    };
    let json = serde_json::to_string_pretty(&out).expect("TileMapOut serializes infallibly");
    fs::write(path, json)
}

pub fn sanitize_name(stem: &str) -> String {
    let mut out: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.chars().next().unwrap().is_ascii_digit() {
        out.insert_str(0, "tile_");
    }
    out
}

pub fn load_png(path: &Path) -> Result<image::DynamicImage, ConvertError> {
    image::open(path).map_err(|e| ConvertError::Image(e.to_string()))
}

pub fn dedup(tiles: &[Tile]) -> DedupResult {
    dedup_tiles(tiles)
}

pub fn summary_line(converted_count: usize, dedup: &DedupResult) -> String {
    format!(
        "{} source tiles -> {} unique after dedup, {} reused via flip, {} rotation-equivalent but not merged",
        converted_count,
        dedup.unique_tiles.len(),
        dedup.reused_via_flip_count(),
        dedup.rotations.len()
    )
}
