use std::path::Path;

use forge_png2tile::{convert, parse_palette_file, ConvertError};
use image::{ImageBuffer, Rgb, RgbImage, Rgba, RgbaImage};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn solid_rgb(width: u32, height: u32, color: [u8; 3]) -> RgbImage {
    ImageBuffer::from_fn(width, height, |_, _| Rgb(color))
}

#[test]
fn rejects_width_not_multiple_of_8() {
    let img = solid_rgb(9, 8, [255, 255, 255]);
    let err = convert(&img, None).unwrap_err();
    assert!(matches!(
        err,
        ConvertError::DimensionsNotMultipleOf8 {
            width: 9,
            height: 8
        }
    ));
}

#[test]
fn rejects_height_not_multiple_of_8() {
    let img = solid_rgb(8, 15, [255, 255, 255]);
    let err = convert(&img, None).unwrap_err();
    assert!(matches!(
        err,
        ConvertError::DimensionsNotMultipleOf8 {
            width: 8,
            height: 15
        }
    ));
}

#[test]
fn rejects_more_than_4_distinct_colors_with_no_palette() {
    // 8x8, one distinct color per pixel-ish: build 5 clearly separated colors.
    let colors: [[u8; 3]; 5] = [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 0],
        [0, 255, 255],
    ];
    let img = ImageBuffer::from_fn(8, 8, |x, _y| Rgb(colors[(x as usize) % colors.len()]));
    let err = convert(&img, None).unwrap_err();
    assert!(matches!(err, ConvertError::TooManyColors { .. }));
}

#[test]
fn rejects_color_not_matching_explicit_palette() {
    let palette = parse_palette_file(&fixture_palette()).unwrap();
    // Bright red is nowhere near the grayscale palette.
    let img = solid_rgb(8, 8, [255, 0, 0]);
    let err = convert(&img, Some(palette)).unwrap_err();
    assert!(matches!(err, ConvertError::ColorNotInPalette { .. }));
}

#[test]
fn accepts_exactly_4_colors_with_no_palette() {
    let colors: [[u8; 3]; 4] = [[255, 255, 255], [170, 170, 170], [85, 85, 85], [0, 0, 0]];
    let img = ImageBuffer::from_fn(8, 8, |x, _y| Rgb(colors[(x as usize) % 4]));
    let result = convert(&img, None).unwrap();
    assert_eq!(result.tiles.len(), 1);
}

#[test]
fn palette_file_parses_4_lines() {
    let palette = parse_palette_file(&fixture_palette()).unwrap();
    assert_eq!(palette.colors[0], forge_core::Rgb::new(255, 255, 255));
    assert_eq!(palette.colors[3], forge_core::Rgb::new(0, 0, 0));
}

#[test]
fn palette_file_rejects_wrong_line_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad_palette.txt");
    std::fs::write(&path, "255 255 255\n0 0 0\n").unwrap();
    let err = parse_palette_file(&path).unwrap_err();
    assert!(matches!(err, ConvertError::PaletteFile(_)));
}

fn fixture_palette() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("palette.txt");
    std::fs::write(&path, "255 255 255\n170 170 170\n85 85 85\n0 0 0\n").unwrap();
    // Leak the tempdir so the path stays valid for the caller; tests are
    // short-lived processes so this is fine.
    std::mem::forget(dir);
    path
}

#[test]
fn flatten_rgba_over_white_composites_correctly() {
    let img: RgbaImage = ImageBuffer::from_fn(2, 2, |_, _| Rgba([0, 0, 0, 128]));
    let flattened = forge_png2tile::flatten_rgba_over_white(&img);
    // 50% black over white ~ mid-gray (128*0 + 127*255)/255 ~ 127
    let px = flattened.get_pixel(0, 0);
    assert!((120..=135).contains(&px[0]), "got {px:?}");
}

#[test]
fn two_bpp_output_matches_real_rgbgfx_reference() {
    // Pre-generated via `rgbgfx -c '#ffffff,#aaaaaa,#555555,#000000' -o ref.2bpp verify.png`
    // against tests/fixtures/verify.png, with the matching palette file. See
    // the task report for the exact command used to produce this fixture.
    let img = image::open(fixture("verify.png")).unwrap().to_rgb8();
    let palette = parse_palette_file(&fixture_palette_matching_verify()).unwrap();
    let converted = convert(&img, Some(palette)).unwrap();

    let expected = std::fs::read(fixture("verify.rgbgfx_reference.2bpp")).unwrap();
    let mut actual = Vec::new();
    for t in &converted.tiles {
        actual.extend_from_slice(&t.to_2bpp_bytes());
    }
    assert_eq!(actual, expected, "must match rgbgfx byte-for-byte");
}

fn fixture_palette_matching_verify() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("verify_palette.txt");
    std::fs::write(&path, "255 255 255\n170 170 170\n85 85 85\n0 0 0\n").unwrap();
    std::mem::forget(dir);
    path
}

#[test]
fn sprite_sheet_dedup_matches_designed_layout() {
    let img = image::open(fixture("sprite_sheet.png")).unwrap().to_rgb8();
    let converted = convert(&img, None).unwrap();
    assert_eq!(converted.tiles.len(), 16);
    let dedup = forge_core::dedup_tiles(&converted.tiles);
    assert_eq!(
        dedup.unique_tiles.len(),
        4,
        "4 unique: F, solid black, A, solid white"
    );
    assert_eq!(dedup.reused_via_flip_count(), 12);
}
