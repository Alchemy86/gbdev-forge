use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use forge_pack::{
    pack_map, pack_meta_sprite, parse_map_decl, parse_meta_sprite_decl, parse_tile_map_ref,
    write_map_json, write_meta_sprite_json, PackError,
};

/// Map / meta-sprite packer: turns a declared layout plus `forge-png2tile`'s
/// tile-reference output into compact VRAM/OAM-ready index arrays.
#[derive(Parser)]
#[command(name = "forge-pack", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pack a declared rows x columns tile layout into a tile-ID index array.
    Map {
        /// JSON map declaration: {"rows", "cols", "tiles": [tile ids, row-major]}.
        decl: PathBuf,
        /// Optional `forge-png2tile --out-map` JSON, used to bounds-check tile ids.
        #[arg(long)]
        tile_map: Option<PathBuf>,
        /// Output JSON path. Defaults to the declaration path with ".packed.json" appended.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Pack a named meta-sprite declaration into (tileId, offsetX, offsetY, flipFlags) tuples.
    Sprite {
        /// JSON meta-sprite declaration: {"name", "tiles": [{"tile_id","offset_x","offset_y","flip"}]}.
        decl: PathBuf,
        /// Optional `forge-png2tile --out-map` JSON, used to bounds-check tile ids.
        #[arg(long)]
        tile_map: Option<PathBuf>,
        /// Output JSON path. Defaults to the declaration path with ".packed.json" appended.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn default_out(decl: &std::path::Path) -> PathBuf {
    let mut out = decl.as_os_str().to_owned();
    out.push(".packed.json");
    PathBuf::from(out)
}

fn run(cli: &Cli) -> Result<(), PackError> {
    match &cli.command {
        Command::Map {
            decl,
            tile_map,
            out,
        } => {
            let decl = parse_map_decl(decl)?;
            let reference = match tile_map {
                Some(p) => Some(parse_tile_map_ref(p)?),
                None => None,
            };
            let output = pack_map(&decl, reference.as_ref())?;
            eprintln!(
                "packed {}x{} map -> {} tile-ID bytes",
                output.rows,
                output.cols,
                output.tile_ids.len()
            );
            let out_path = out.clone().unwrap_or_else(|| default_out(decl_path(cli)));
            write_map_json(&out_path, &output)?;
        }
        Command::Sprite {
            decl,
            tile_map,
            out,
        } => {
            let decl = parse_meta_sprite_decl(decl)?;
            let reference = match tile_map {
                Some(p) => Some(parse_tile_map_ref(p)?),
                None => None,
            };
            let output = pack_meta_sprite(&decl, reference.as_ref())?;
            eprintln!(
                "packed meta-sprite \"{}\" -> {} tile(s)",
                output.name,
                output.tiles.len()
            );
            let out_path = out.clone().unwrap_or_else(|| default_out(decl_path(cli)));
            write_meta_sprite_json(&out_path, &output)?;
        }
    }
    Ok(())
}

/// Helper so the default-output path can be derived from whichever
/// subcommand's `decl` path was given, without borrowing issues above.
fn decl_path(cli: &Cli) -> &std::path::Path {
    match &cli.command {
        Command::Map { decl, .. } => decl,
        Command::Sprite { decl, .. } => decl,
    }
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
