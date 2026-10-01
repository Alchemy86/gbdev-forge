use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use forge_bank::{allocate, parse_manifest, write_bank_plan_json, write_linker_script, BankError};

/// MBC bank allocator: packs a JSON manifest into 16KB ROM banks and emits
/// a bank map plus an RGBDS-compatible linker-script fragment.
#[derive(Parser)]
#[command(name = "forge-bank", version, about)]
struct Cli {
    /// JSON manifest: {"mbc", "cart_size", "items": [{"name","size","bank"}]}.
    manifest: PathBuf,

    /// Output bank-map JSON path. Defaults to the manifest path with
    /// ".banks.json" appended.
    #[arg(long)]
    out_map: Option<PathBuf>,

    /// Output RGBDS linker-script fragment path. Defaults to the manifest
    /// path with ".sections.inc" appended.
    #[arg(long)]
    out_link: Option<PathBuf>,
}

fn run(cli: &Cli) -> Result<(), BankError> {
    let manifest = parse_manifest(&cli.manifest)?;
    let plan = allocate(&manifest)?;

    eprintln!(
        "{} banks allocated for {}, {} byte(s) cartridge",
        plan.total_banks,
        plan.mbc,
        plan.banks.iter().map(|b| b.capacity).sum::<u32>()
    );
    for bank in &plan.banks {
        if !bank.items.is_empty() {
            eprintln!(
                "  bank {}: {}/{} bytes used, {} item(s)",
                bank.id,
                bank.used,
                bank.capacity,
                bank.items.len()
            );
        }
    }

    let mut out_map = cli.manifest.clone().into_os_string();
    out_map.push(".banks.json");
    let out_map = cli.out_map.clone().unwrap_or_else(|| PathBuf::from(out_map));
    write_bank_plan_json(&out_map, &plan)?;

    let mut out_link = cli.manifest.clone().into_os_string();
    out_link.push(".sections.inc");
    let out_link = cli
        .out_link
        .clone()
        .unwrap_or_else(|| PathBuf::from(out_link));
    write_linker_script(&out_link, &plan)?;

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
