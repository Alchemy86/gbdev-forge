use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use forge_png2tile::{
    convert, dedup, flatten_rgba_over_white, load_png, parse_palette_file, sanitize_name,
    summary_line, write_2bpp, write_asm_array, write_c_array, write_tile_map_json, ConvertError,
};

/// PNG -> Game Boy 2BPP tile converter with flip-aware smart dedup.
#[derive(Parser)]
#[command(name = "forge-png2tile", version, about)]
struct Cli {
    /// Input PNG file. Width and height must each be a multiple of 8.
    input: PathBuf,

    /// Optional palette file: exactly 4 lines of "R G B" (0-255 decimal).
    /// Without this, colors are auto-quantized by luminance into up to 4
    /// shades (index 0 = brightest).
    #[arg(long)]
    palette: Option<PathBuf>,

    /// Required for any PNG with an alpha channel: composites it over a
    /// white background before quantizing.
    #[arg(long)]
    flatten_alpha: bool,

    /// Output raw .2bpp binary (unique tiles only, first-seen order).
    /// Defaults to the input path with its extension replaced by .2bpp.
    #[arg(long)]
    out_2bpp: Option<PathBuf>,

    /// Also emit a `const uint8_t NAME[] = {...};` C hex array.
    #[arg(long)]
    out_c: Option<PathBuf>,

    /// Also emit a `DB $xx,$xx,...` RGBDS hex array.
    #[arg(long)]
    out_asm: Option<PathBuf>,

    /// Also emit the tile-reference map (source tile -> unique id + flip,
    /// plus rotation findings) as JSON.
    #[arg(long)]
    out_map: Option<PathBuf>,

    /// Array/label name used in --out-c / --out-asm output. Defaults to a
    /// sanitized form of the input file's stem.
    #[arg(long)]
    name: Option<String>,
}

fn run(cli: &Cli) -> Result<(), ConvertError> {
    let img = load_png(&cli.input)?;
    let (width, height) = (
        image::GenericImageView::width(&img),
        image::GenericImageView::height(&img),
    );
    if width % 8 != 0 || height % 8 != 0 {
        return Err(ConvertError::DimensionsNotMultipleOf8 { width, height });
    }

    let has_alpha = img.color().has_alpha();
    if has_alpha && !cli.flatten_alpha {
        return Err(ConvertError::AlphaWithoutFlatten);
    }
    let rgb_img = if has_alpha {
        flatten_rgba_over_white(&img.to_rgba8())
    } else {
        img.to_rgb8()
    };

    let palette = match &cli.palette {
        Some(p) => Some(parse_palette_file(p)?),
        None => None,
    };

    let converted = convert(&rgb_img, palette)?;
    let dedup_result = dedup(&converted.tiles);

    eprintln!("{}", summary_line(converted.tiles.len(), &dedup_result));
    if !dedup_result.rotations.is_empty() {
        eprintln!(
            "note: {} tile(s) are 90/270-degree rotations of existing tiles and were NOT merged - real hardware has no rotate flag",
            dedup_result.rotations.len()
        );
    }
    eprintln!(
        "note: flip-based reuse is valid for sprites (OAM X/Y flip bits) only - DMG background tiles have no flip capability (that's a GBC-only BG attribute)"
    );

    let out_2bpp = cli
        .out_2bpp
        .clone()
        .unwrap_or_else(|| cli.input.with_extension("2bpp"));
    write_2bpp(&out_2bpp, &dedup_result.unique_tiles)?;

    let name = cli.name.clone().unwrap_or_else(|| {
        sanitize_name(
            cli.input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("tiles"),
        )
    });
    if let Some(p) = &cli.out_c {
        write_c_array(p, &name, &dedup_result.unique_tiles)?;
    }
    if let Some(p) = &cli.out_asm {
        write_asm_array(p, &name, &dedup_result.unique_tiles)?;
    }
    if let Some(p) = &cli.out_map {
        write_tile_map_json(p, &dedup_result)?;
    }

    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
