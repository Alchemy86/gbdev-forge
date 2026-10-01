//! Packs `forge-png2tile`'s de-duplicated unique-tile output into the two
//! shapes real Game Boy VRAM/OAM actually consume: a background tile-map
//! index array, or a meta-sprite's (tileId, offsetX, offsetY, flipFlags)
//! tuple array. Neither output re-flattens pixel data -- VRAM tile-maps and
//! OAM entries both hold indices, not pixels.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use forge_core::Flip;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum PackError {
    /// Mismatch between a declared map's `rows * cols` and its `tiles` entry count.
    MapSizeMismatch { expected: usize, got: usize },
    /// A declared tile id doesn't fit in a single VRAM tile-map byte (0-255).
    TileIdTooLarge { tile_id: usize },
    /// A declared tile id exceeds the referenced tile-map's unique tile count.
    TileIdOutOfRange { tile_id: usize, unique_tile_count: usize },
    /// An unrecognized flip string in a meta-sprite declaration.
    InvalidFlip { value: String },
    Json(String),
    Io(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::MapSizeMismatch { expected, got } => write!(
                f,
                "map declares rows*cols = {expected} but its tiles array has {got} entries"
            ),
            PackError::TileIdTooLarge { tile_id } => write!(
                f,
                "tile id {tile_id} does not fit in a single VRAM tile-map byte (0-255)"
            ),
            PackError::TileIdOutOfRange {
                tile_id,
                unique_tile_count,
            } => write!(
                f,
                "tile id {tile_id} is out of range: referenced tile map has only {unique_tile_count} unique tile(s)"
            ),
            PackError::InvalidFlip { value } => write!(
                f,
                "invalid flip value \"{value}\": expected one of \"none\", \"h\", \"v\", \"hv\""
            ),
            PackError::Json(msg) => write!(f, "invalid JSON: {msg}"),
            PackError::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for PackError {}

impl From<io::Error> for PackError {
    fn from(e: io::Error) -> Self {
        PackError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for PackError {
    fn from(e: serde_json::Error) -> Self {
        PackError::Json(e.to_string())
    }
}

/// The subset of `forge-png2tile`'s `--out-map` JSON this tool needs to
/// bounds-check declared tile ids against. Extra fields in the source file
/// are ignored by `serde_json`.
#[derive(Deserialize)]
pub struct TileMapRef {
    pub unique_tile_count: usize,
}

pub fn parse_tile_map_ref(path: &Path) -> Result<TileMapRef, PackError> {
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn parse_flip(value: &str) -> Result<Flip, PackError> {
    match value {
        "none" | "" => Ok(Flip::None),
        "h" => Ok(Flip::Horizontal),
        "v" => Ok(Flip::Vertical),
        "hv" => Ok(Flip::Both),
        other => Err(PackError::InvalidFlip {
            value: other.to_string(),
        }),
    }
}

/// OAM attribute byte flip bits: bit 5 (0x20) = X flip, bit 6 (0x40) = Y flip.
fn flip_flags_byte(flip: Flip) -> u8 {
    match flip {
        Flip::None => 0x00,
        Flip::Horizontal => 0x20,
        Flip::Vertical => 0x40,
        Flip::Both => 0x60,
    }
}

fn check_tile_id(tile_id: usize, reference: Option<&TileMapRef>) -> Result<u8, PackError> {
    if tile_id > u8::MAX as usize {
        return Err(PackError::TileIdTooLarge { tile_id });
    }
    if let Some(reference) = reference {
        if tile_id >= reference.unique_tile_count {
            return Err(PackError::TileIdOutOfRange {
                tile_id,
                unique_tile_count: reference.unique_tile_count,
            });
        }
    }
    Ok(tile_id as u8)
}

// ---- Map ----

/// A declared rows x columns tile placement, tile ids in row-major order.
#[derive(Deserialize)]
pub struct MapDecl {
    pub rows: usize,
    pub cols: usize,
    pub tiles: Vec<usize>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MapOutput {
    pub rows: usize,
    pub cols: usize,
    /// Row-major VRAM tile-map indices -- one byte per entry, no pixel data.
    pub tile_ids: Vec<u8>,
}

pub fn parse_map_decl(path: &Path) -> Result<MapDecl, PackError> {
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

pub fn pack_map(decl: &MapDecl, reference: Option<&TileMapRef>) -> Result<MapOutput, PackError> {
    let expected = decl.rows * decl.cols;
    if decl.tiles.len() != expected {
        return Err(PackError::MapSizeMismatch {
            expected,
            got: decl.tiles.len(),
        });
    }
    let mut tile_ids = Vec::with_capacity(decl.tiles.len());
    for &tile_id in &decl.tiles {
        tile_ids.push(check_tile_id(tile_id, reference)?);
    }
    Ok(MapOutput {
        rows: decl.rows,
        cols: decl.cols,
        tile_ids,
    })
}

pub fn write_map_json(path: &Path, output: &MapOutput) -> io::Result<()> {
    let json = serde_json::to_string_pretty(output).expect("MapOutput serializes infallibly");
    fs::write(path, json)
}

// ---- Meta-sprite ----

#[derive(Deserialize)]
pub struct MetaSpriteTileDecl {
    pub tile_id: usize,
    pub offset_x: i16,
    pub offset_y: i16,
    #[serde(default)]
    pub flip: String,
}

#[derive(Deserialize)]
pub struct MetaSpriteDecl {
    pub name: String,
    pub tiles: Vec<MetaSpriteTileDecl>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MetaSpriteTileOutput {
    pub tile_id: u8,
    pub offset_x: i16,
    pub offset_y: i16,
    /// OAM-style attribute flip bits: 0x20 = X flip, 0x40 = Y flip.
    pub flip_flags: u8,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MetaSpriteOutput {
    pub name: String,
    pub tiles: Vec<MetaSpriteTileOutput>,
}

pub fn parse_meta_sprite_decl(path: &Path) -> Result<MetaSpriteDecl, PackError> {
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

pub fn pack_meta_sprite(
    decl: &MetaSpriteDecl,
    reference: Option<&TileMapRef>,
) -> Result<MetaSpriteOutput, PackError> {
    let mut tiles = Vec::with_capacity(decl.tiles.len());
    for t in &decl.tiles {
        let tile_id = check_tile_id(t.tile_id, reference)?;
        let flip = parse_flip(&t.flip)?;
        tiles.push(MetaSpriteTileOutput {
            tile_id,
            offset_x: t.offset_x,
            offset_y: t.offset_y,
            flip_flags: flip_flags_byte(flip),
        });
    }
    Ok(MetaSpriteOutput {
        name: decl.name.clone(),
        tiles,
    })
}

pub fn write_meta_sprite_json(path: &Path, output: &MetaSpriteOutput) -> io::Result<()> {
    let json =
        serde_json::to_string_pretty(output).expect("MetaSpriteOutput serializes infallibly");
    fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_a_small_synthetic_map() {
        let decl = MapDecl {
            rows: 2,
            cols: 3,
            tiles: vec![0, 1, 2, 2, 1, 0],
        };
        let output = pack_map(&decl, None).unwrap();
        assert_eq!(
            output,
            MapOutput {
                rows: 2,
                cols: 3,
                tile_ids: vec![0, 1, 2, 2, 1, 0],
            }
        );
    }

    #[test]
    fn map_rejects_size_mismatch() {
        let decl = MapDecl {
            rows: 2,
            cols: 3,
            tiles: vec![0, 1, 2],
        };
        let err = pack_map(&decl, None).unwrap_err();
        assert!(matches!(
            err,
            PackError::MapSizeMismatch {
                expected: 6,
                got: 3
            }
        ));
    }

    #[test]
    fn map_rejects_tile_id_out_of_reference_range() {
        let decl = MapDecl {
            rows: 1,
            cols: 2,
            tiles: vec![0, 5],
        };
        let reference = TileMapRef {
            unique_tile_count: 3,
        };
        let err = pack_map(&decl, Some(&reference)).unwrap_err();
        assert!(matches!(
            err,
            PackError::TileIdOutOfRange {
                tile_id: 5,
                unique_tile_count: 3
            }
        ));
    }

    #[test]
    fn packs_a_small_synthetic_meta_sprite() {
        let decl = MetaSpriteDecl {
            name: "player".to_string(),
            tiles: vec![
                MetaSpriteTileDecl {
                    tile_id: 0,
                    offset_x: 0,
                    offset_y: 0,
                    flip: "none".to_string(),
                },
                MetaSpriteTileDecl {
                    tile_id: 1,
                    offset_x: 8,
                    offset_y: 0,
                    flip: "h".to_string(),
                },
                MetaSpriteTileDecl {
                    tile_id: 0,
                    offset_x: 0,
                    offset_y: 8,
                    flip: "v".to_string(),
                },
                MetaSpriteTileDecl {
                    tile_id: 1,
                    offset_x: 8,
                    offset_y: 8,
                    flip: "hv".to_string(),
                },
            ],
        };
        let output = pack_meta_sprite(&decl, None).unwrap();
        assert_eq!(
            output,
            MetaSpriteOutput {
                name: "player".to_string(),
                tiles: vec![
                    MetaSpriteTileOutput {
                        tile_id: 0,
                        offset_x: 0,
                        offset_y: 0,
                        flip_flags: 0x00,
                    },
                    MetaSpriteTileOutput {
                        tile_id: 1,
                        offset_x: 8,
                        offset_y: 0,
                        flip_flags: 0x20,
                    },
                    MetaSpriteTileOutput {
                        tile_id: 0,
                        offset_x: 0,
                        offset_y: 8,
                        flip_flags: 0x40,
                    },
                    MetaSpriteTileOutput {
                        tile_id: 1,
                        offset_x: 8,
                        offset_y: 8,
                        flip_flags: 0x60,
                    },
                ],
            }
        );
    }

    #[test]
    fn meta_sprite_rejects_invalid_flip() {
        let decl = MetaSpriteDecl {
            name: "bad".to_string(),
            tiles: vec![MetaSpriteTileDecl {
                tile_id: 0,
                offset_x: 0,
                offset_y: 0,
                flip: "diagonal".to_string(),
            }],
        };
        let err = pack_meta_sprite(&decl, None).unwrap_err();
        assert!(matches!(err, PackError::InvalidFlip { .. }));
    }

    #[test]
    fn rejects_tile_id_that_does_not_fit_in_a_byte() {
        let decl = MapDecl {
            rows: 1,
            cols: 1,
            tiles: vec![256],
        };
        let err = pack_map(&decl, None).unwrap_err();
        assert!(matches!(err, PackError::TileIdTooLarge { tile_id: 256 }));
    }
}
