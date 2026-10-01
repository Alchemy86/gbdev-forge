//! Shared Tile/Palette data model for the gbdev-forge toolchain.
//!
//! Kept target-agnostic (no GBC/GBA-specific concerns beyond the 2BPP
//! encoder, which is the one piece of hardware-exact behavior every
//! consumer -- including a future debugger/VRAM inspector -- needs to
//! agree on bit-for-bit) so later tools can reuse these types without
//! rework.

use serde::Serialize;
use std::collections::HashMap;

/// An 8x8 block of 2-bit palette indices (0..=3), in row-major order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tile {
    pub pixels: [[u8; 8]; 8],
}

/// The four transforms real Game Boy hardware can apply for free via a
/// sprite's OAM X/Y flip attribute bits. DMG background tiles have no flip
/// capability at all -- that's a GBC-only BG attribute -- so callers
/// targeting a DMG background layer should treat anything but `None` as
/// unusable there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Flip {
    None,
    Horizontal,
    Vertical,
    Both,
}

impl Flip {
    pub fn as_str(&self) -> &'static str {
        match self {
            Flip::None => "none",
            Flip::Horizontal => "h",
            Flip::Vertical => "v",
            Flip::Both => "hv",
        }
    }
}

/// A 90/270-degree rotation relationship. Hardware has no rotate flag, so a
/// match here is reported but never merged into the unique-tile set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Rotation {
    R90,
    R270,
}

impl Rotation {
    pub fn degrees(&self) -> u16 {
        match self {
            Rotation::R90 => 90,
            Rotation::R270 => 270,
        }
    }
}

impl Tile {
    pub fn flip_horizontal(&self) -> Tile {
        let mut pixels = self.pixels;
        for row in &mut pixels {
            row.reverse();
        }
        Tile { pixels }
    }

    pub fn flip_vertical(&self) -> Tile {
        let mut pixels = self.pixels;
        pixels.reverse();
        Tile { pixels }
    }

    pub fn flip_both(&self) -> Tile {
        self.flip_horizontal().flip_vertical()
    }

    pub fn transformed(&self, flip: Flip) -> Tile {
        match flip {
            Flip::None => *self,
            Flip::Horizontal => self.flip_horizontal(),
            Flip::Vertical => self.flip_vertical(),
            Flip::Both => self.flip_both(),
        }
    }

    /// Clockwise 90-degree rotation.
    #[allow(clippy::needless_range_loop)]
    pub fn rotate90(&self) -> Tile {
        let mut pixels = [[0u8; 8]; 8];
        for y in 0..8 {
            for x in 0..8 {
                pixels[x][7 - y] = self.pixels[y][x];
            }
        }
        Tile { pixels }
    }

    /// Clockwise 270-degree rotation (i.e. counter-clockwise 90).
    #[allow(clippy::needless_range_loop)]
    pub fn rotate270(&self) -> Tile {
        let mut pixels = [[0u8; 8]; 8];
        for y in 0..8 {
            for x in 0..8 {
                pixels[7 - x][y] = self.pixels[y][x];
            }
        }
        Tile { pixels }
    }

    /// Encode this tile to its 16-byte Game Boy 2BPP representation.
    ///
    /// Per row: a low-plane byte then a high-plane byte, MSB-first per
    /// pixel (bit 7 = leftmost pixel). Matches `rgbgfx -o` byte-for-byte.
    pub fn to_2bpp_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (row_idx, row) in self.pixels.iter().enumerate() {
            let mut low = 0u8;
            let mut high = 0u8;
            for (col_idx, &index) in row.iter().enumerate() {
                let shift = 7 - col_idx;
                low |= (index & 1) << shift;
                high |= ((index >> 1) & 1) << shift;
            }
            out[row_idx * 2] = low;
            out[row_idx * 2 + 1] = high;
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// ITU-R BT.601 luma approximation, used to order auto-quantized
    /// shades and for nearest-color matching.
    pub fn luminance(&self) -> f32 {
        0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32
    }

    pub fn distance(&self, other: &Rgb) -> f32 {
        let dr = self.r as f32 - other.r as f32;
        let dg = self.g as f32 - other.g as f32;
        let db = self.b as f32 - other.b as f32;
        (dr * dr + dg * dg + db * db).sqrt()
    }
}

/// A 4-shade Game Boy palette, index 0..=3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub colors: [Rgb; 4],
}

/// Which unique tile (and flip) a source tile position maps to.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TileRef {
    pub unique_id: usize,
    pub flip: Flip,
}

/// A source tile that is a 90/270-degree rotation of an already-registered
/// unique tile. Reported, never merged.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RotationFinding {
    pub source_index: usize,
    pub matches_unique_id: usize,
    pub rotation: Rotation,
}

#[derive(Debug, Clone)]
pub struct DedupResult {
    /// Unique tiles, in first-seen order.
    pub unique_tiles: Vec<Tile>,
    /// One entry per source tile, in source order.
    pub references: Vec<TileRef>,
    /// Rotation-only matches, not merged.
    pub rotations: Vec<RotationFinding>,
}

impl DedupResult {
    pub fn reused_via_flip_count(&self) -> usize {
        self.references.len() - self.unique_tiles.len()
    }
}

/// Deduplicate source tiles under the four hardware-free transforms
/// (identity, horizontal flip, vertical flip, both), and separately report
/// 90/270-degree rotation matches without merging them.
pub fn dedup_tiles(tiles: &[Tile]) -> DedupResult {
    // Maps a pixel grid to the (unique_id, flip) that reproduces it, i.e.
    // key == unique_tiles[unique_id].transformed(flip).
    let mut variant_map: HashMap<[[u8; 8]; 8], (usize, Flip)> = HashMap::new();
    // Maps `existing.rotateX().pixels` -> (unique_id, X), kept separate from
    // variant_map since rotation matches are reported, never merged.
    let mut rotation_map: HashMap<[[u8; 8]; 8], (usize, Rotation)> = HashMap::new();
    let mut unique_tiles: Vec<Tile> = Vec::new();
    let mut references: Vec<TileRef> = Vec::with_capacity(tiles.len());
    let mut rotations: Vec<RotationFinding> = Vec::new();

    for (source_index, tile) in tiles.iter().enumerate() {
        if let Some(&(unique_id, flip)) = variant_map.get(&tile.pixels) {
            references.push(TileRef { unique_id, flip });
            continue;
        }

        // Not reachable via flip from anything seen so far: check rotation
        // (report-only) against what's registered before adding this tile.
        if let Some(&(unique_id, rotation)) = rotation_map.get(&tile.pixels) {
            rotations.push(RotationFinding {
                source_index,
                matches_unique_id: unique_id,
                rotation,
            });
        }

        let unique_id = unique_tiles.len();
        for flip in [Flip::None, Flip::Horizontal, Flip::Vertical, Flip::Both] {
            variant_map
                .entry(tile.transformed(flip).pixels)
                .or_insert((unique_id, flip));
        }
        rotation_map
            .entry(tile.rotate90().pixels)
            .or_insert((unique_id, Rotation::R90));
        rotation_map
            .entry(tile.rotate270().pixels)
            .or_insert((unique_id, Rotation::R270));
        unique_tiles.push(*tile);
        references.push(TileRef {
            unique_id,
            flip: Flip::None,
        });
    }

    DedupResult {
        unique_tiles,
        references,
        rotations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(index: u8) -> Tile {
        Tile {
            pixels: [[index; 8]; 8],
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn checker() -> Tile {
        let mut pixels = [[0u8; 8]; 8];
        for y in 0..8 {
            for x in 0..8 {
                pixels[y][x] = if (x + y) % 2 == 0 { 3 } else { 0 };
            }
        }
        Tile { pixels }
    }

    fn asymmetric() -> Tile {
        // A letter-"F" glyph: has no symmetry under any of the 8 dihedral
        // transforms (identity, 2 rotations, 2 diagonal flips, h/v flip,
        // 180), so every transform below is guaranteed distinct from
        // every other.
        let mut pixels = [[0u8; 8]; 8];
        pixels[0] = [3, 3, 3, 3, 0, 0, 0, 0];
        pixels[1] = [3, 0, 0, 0, 0, 0, 0, 0];
        pixels[2] = [3, 3, 3, 0, 0, 0, 0, 0];
        pixels[3] = [3, 0, 0, 0, 0, 0, 0, 0];
        pixels[4] = [3, 0, 0, 0, 0, 0, 0, 0];
        Tile { pixels }
    }

    #[test]
    fn two_bpp_encoding_matches_spec_worked_example() {
        // Row of pixel indices [3,3,0,0,1,1,2,2] (leftmost = MSB):
        // low  bits: 1,1,0,0,1,1,0,0 -> 0b11001100 = 0xCC
        // high bits: 1,1,0,0,0,0,1,1 -> 0b11000011 = 0xC3
        let mut pixels = [[0u8; 8]; 8];
        pixels[0] = [3, 3, 0, 0, 1, 1, 2, 2];
        let tile = Tile { pixels };
        let bytes = tile.to_2bpp_bytes();
        assert_eq!(bytes[0], 0xCC, "low-plane byte");
        assert_eq!(bytes[1], 0xC3, "high-plane byte");
    }

    #[test]
    fn two_bpp_all_zero_tile_is_all_zero_bytes() {
        assert_eq!(solid(0).to_2bpp_bytes(), [0u8; 16]);
    }

    #[test]
    fn two_bpp_all_index3_tile_is_all_0xff() {
        assert_eq!(solid(3).to_2bpp_bytes(), [0xFFu8; 16]);
    }

    #[test]
    fn flip_horizontal_reverses_columns() {
        let t = asymmetric();
        let flipped = t.flip_horizontal();
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(flipped.pixels[y][x], t.pixels[y][7 - x]);
            }
        }
    }

    #[test]
    fn flip_vertical_reverses_rows() {
        let t = asymmetric();
        let flipped = t.flip_vertical();
        for y in 0..8 {
            assert_eq!(flipped.pixels[y], t.pixels[7 - y]);
        }
    }

    #[test]
    fn dedup_identity_duplicate_tiles_collapse_to_one() {
        let tiles = vec![checker(), checker(), checker()];
        let result = dedup_tiles(&tiles);
        assert_eq!(result.unique_tiles.len(), 1);
        assert_eq!(result.references.len(), 3);
        assert!(result.references.iter().all(|r| r.unique_id == 0));
    }

    #[test]
    fn dedup_solid_color_blocks_collapse_regardless_of_position() {
        // The captain's literal example: two differently-positioned solid
        // black 8x8 regions collapse to one unique tile.
        let tiles = vec![solid(3), checker(), solid(3)];
        let result = dedup_tiles(&tiles);
        assert_eq!(result.unique_tiles.len(), 2, "solid tiles must merge");
        assert_eq!(
            result.references[0].unique_id,
            result.references[2].unique_id
        );
        assert_eq!(result.references[0].flip.as_str(), "none");
    }

    #[test]
    fn dedup_merges_horizontal_flip() {
        let base = asymmetric();
        let tiles = vec![base, base.flip_horizontal()];
        let result = dedup_tiles(&tiles);
        assert_eq!(result.unique_tiles.len(), 1);
        assert_eq!(result.references[1].unique_id, 0);
        assert!(matches!(result.references[1].flip, Flip::Horizontal));
    }

    #[test]
    fn dedup_merges_vertical_flip() {
        let base = asymmetric();
        let tiles = vec![base, base.flip_vertical()];
        let result = dedup_tiles(&tiles);
        assert_eq!(result.unique_tiles.len(), 1);
        assert!(matches!(result.references[1].flip, Flip::Vertical));
    }

    #[test]
    fn dedup_merges_both_flip() {
        let base = asymmetric();
        let tiles = vec![base, base.flip_both()];
        let result = dedup_tiles(&tiles);
        assert_eq!(result.unique_tiles.len(), 1);
        assert!(matches!(result.references[1].flip, Flip::Both));
    }

    #[test]
    fn dedup_reports_before_after_counts_correctly() {
        let base = asymmetric();
        let tiles = vec![base, base.flip_horizontal(), checker(), checker()];
        let result = dedup_tiles(&tiles);
        assert_eq!(tiles.len(), 4, "source tile count");
        assert_eq!(result.unique_tiles.len(), 2, "unique after dedup");
        assert_eq!(result.reused_via_flip_count(), 2, "reused via flip");
    }

    #[test]
    fn rotation_match_is_reported_but_not_merged() {
        let base = asymmetric();
        let rotated = base.rotate90();
        let tiles = vec![base, rotated];
        let result = dedup_tiles(&tiles);
        // Hardware has no rotate flag: rotated tile must remain its own
        // unique tile, not merged into the base.
        assert_eq!(result.unique_tiles.len(), 2);
        assert_eq!(result.references[1].unique_id, 1);
        assert_eq!(result.rotations.len(), 1);
        assert_eq!(result.rotations[0].source_index, 1);
        assert_eq!(result.rotations[0].matches_unique_id, 0);
        assert!(matches!(result.rotations[0].rotation, Rotation::R90));
    }

    #[test]
    fn rotation270_match_is_reported_but_not_merged() {
        let base = asymmetric();
        let rotated = base.rotate270();
        let tiles = vec![base, rotated];
        let result = dedup_tiles(&tiles);
        assert_eq!(result.unique_tiles.len(), 2);
        assert_eq!(result.rotations.len(), 1);
        assert!(matches!(result.rotations[0].rotation, Rotation::R270));
    }
}
