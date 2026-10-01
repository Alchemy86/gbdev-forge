//! Invokes `forge-bank`'s library functions directly (no subprocess) so MBC
//! bank allocation is reachable from inside the IDE, not only the CLI.

use forge_bank::{allocate, render_linker_script, Manifest, ManifestItem};

#[derive(Default)]
pub struct BankPanel {
    pub mbc: String,
    /// Cartridge size in bytes, as text.
    pub cart_size: String,
    /// One "name size [bank]" per line; bank is optional (omit for bankable).
    pub items_input: String,
    pub result: Option<Result<String, String>>,
}

impl BankPanel {
    pub fn run(&mut self) {
        self.result = Some(Self::allocate_from_input(
            &self.mbc,
            &self.cart_size,
            &self.items_input,
        ));
    }

    fn allocate_from_input(mbc: &str, cart_size: &str, items_input: &str) -> Result<String, String> {
        let cart_size: u32 = cart_size
            .trim()
            .parse()
            .map_err(|_| "cart_size must be a whole number of bytes".to_string())?;

        let mut items = Vec::new();
        for line in items_input.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 2 && fields.len() != 3 {
                return Err(format!(
                    "line \"{line}\" must be \"name size\" or \"name size bank\""
                ));
            }
            let size: u32 = fields[1].parse().map_err(|_| "bad size".to_string())?;
            let bank = if fields.len() == 3 {
                Some(fields[2].parse::<u16>().map_err(|_| "bad bank".to_string())?)
            } else {
                None
            };
            items.push(ManifestItem {
                name: fields[0].to_string(),
                size,
                bank,
            });
        }

        let manifest = Manifest {
            mbc: mbc.trim().to_string(),
            cart_size,
            items,
        };
        let plan = allocate(&manifest).map_err(|e| e.to_string())?;

        let mut out = format!(
            "{} banks allocated for {}\n",
            plan.total_banks, plan.mbc
        );
        for bank in &plan.banks {
            if bank.items.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "bank {}: {}/{} bytes -- {}\n",
                bank.id,
                bank.used,
                bank.capacity,
                bank.items
                    .iter()
                    .map(|i| i.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out.push('\n');
        out.push_str(&render_linker_script(&plan));
        Ok(out)
    }
}
