//! MBC bank allocator: partitions a manifest's items into pinned-bank and
//! bankable groups, then greedily packs the bankable ones into 16KB ROM
//! banks (first-fit decreasing by size). Overflow is always a hard error at
//! allocation time, naming exactly which item didn't fit and by how many
//! bytes -- never a silent wrap.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Real Game Boy ROM bank size, true for MBC1/3/5 alike.
pub const BANK_SIZE: u32 = 16 * 1024;

#[derive(Debug)]
pub enum BankError {
    UnknownMbc { value: String },
    /// `cart_size` isn't an exact multiple of `BANK_SIZE`.
    CartSizeNotBankAligned { cart_size: u32 },
    CartSizeExceedsMbcMax { mbc: String, cart_size: u32, max: u32 },
    /// A pinned item names a bank beyond the cartridge's own bank count.
    BankOutOfRange {
        item: String,
        bank: u16,
        total_banks: u16,
    },
    /// An item (pinned or bankable) could not fit anywhere.
    Overflow {
        item: String,
        bank: Option<u16>,
        needed: u32,
        available: u32,
        over_by: u32,
    },
    Json(String),
    Io(String),
}

impl fmt::Display for BankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BankError::UnknownMbc { value } => write!(
                f,
                "unknown MBC type \"{value}\": expected one of \"MBC1\", \"MBC3\", \"MBC5\""
            ),
            BankError::CartSizeNotBankAligned { cart_size } => write!(
                f,
                "cart_size {cart_size} is not an exact multiple of the {BANK_SIZE}-byte bank size"
            ),
            BankError::CartSizeExceedsMbcMax { mbc, cart_size, max } => write!(
                f,
                "cart_size {cart_size} exceeds {mbc}'s maximum addressable ROM size of {max} bytes"
            ),
            BankError::BankOutOfRange {
                item,
                bank,
                total_banks,
            } => write!(
                f,
                "item \"{item}\" is pinned to bank {bank}, but the cartridge only has {total_banks} bank(s) (0..{})",
                total_banks - 1
            ),
            BankError::Overflow {
                item,
                bank: Some(bank),
                needed,
                available,
                over_by,
            } => write!(
                f,
                "item \"{item}\" ({needed} bytes) does not fit in its pinned bank {bank} ({available} bytes available) -- over by {over_by} byte(s)"
            ),
            BankError::Overflow {
                item,
                bank: None,
                needed,
                over_by,
                ..
            } => write!(
                f,
                "item \"{item}\" ({needed} bytes) does not fit in any remaining bank -- over by {over_by} byte(s)"
            ),
            BankError::Json(msg) => write!(f, "invalid JSON: {msg}"),
            BankError::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for BankError {}

impl From<io::Error> for BankError {
    fn from(e: io::Error) -> Self {
        BankError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for BankError {
    fn from(e: serde_json::Error) -> Self {
        BankError::Json(e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mbc {
    Mbc1,
    Mbc3,
    Mbc5,
}

impl Mbc {
    pub fn parse(value: &str) -> Result<Mbc, BankError> {
        match value.to_ascii_uppercase().as_str() {
            "MBC1" => Ok(Mbc::Mbc1),
            "MBC3" => Ok(Mbc::Mbc3),
            "MBC5" => Ok(Mbc::Mbc5),
            other => Err(BankError::UnknownMbc {
                value: other.to_string(),
            }),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mbc::Mbc1 => "MBC1",
            Mbc::Mbc3 => "MBC3",
            Mbc::Mbc5 => "MBC5",
        }
    }

    /// Maximum addressable ROM size, per the MBC's real bank-select register width.
    pub fn max_rom_size(&self) -> u32 {
        match self {
            Mbc::Mbc1 => 2 * 1024 * 1024,
            Mbc::Mbc3 => 2 * 1024 * 1024,
            Mbc::Mbc5 => 8 * 1024 * 1024,
        }
    }
}

#[derive(Deserialize)]
pub struct ManifestItem {
    pub name: String,
    pub size: u32,
    /// Explicit bank pin. Omitted/`null` means "bankable": packed
    /// automatically by the allocator. `0` pins to the fixed home bank.
    #[serde(default)]
    pub bank: Option<u16>,
}

#[derive(Deserialize)]
pub struct Manifest {
    pub mbc: String,
    pub cart_size: u32,
    pub items: Vec<ManifestItem>,
}

pub fn parse_manifest(path: &Path) -> Result<Manifest, BankError> {
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PlacedItem {
    pub name: String,
    pub size: u32,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Bank {
    pub id: u16,
    pub used: u32,
    pub capacity: u32,
    pub items: Vec<PlacedItem>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BankPlan {
    pub mbc: String,
    pub total_banks: u16,
    pub banks: Vec<Bank>,
}

pub fn allocate(manifest: &Manifest) -> Result<BankPlan, BankError> {
    let mbc = Mbc::parse(&manifest.mbc)?;

    if !manifest.cart_size.is_multiple_of(BANK_SIZE) || manifest.cart_size == 0 {
        return Err(BankError::CartSizeNotBankAligned {
            cart_size: manifest.cart_size,
        });
    }
    let max = mbc.max_rom_size();
    if manifest.cart_size > max {
        return Err(BankError::CartSizeExceedsMbcMax {
            mbc: mbc.as_str().to_string(),
            cart_size: manifest.cart_size,
            max,
        });
    }

    let total_banks = (manifest.cart_size / BANK_SIZE) as u16;
    let mut banks: Vec<Bank> = (0..total_banks)
        .map(|id| Bank {
            id,
            used: 0,
            capacity: BANK_SIZE,
            items: Vec::new(),
        })
        .collect();

    // Partition: pinned items (fixed/explicit bank) placed first and
    // exactly where declared; everything else is bankable.
    let (pinned, bankable): (Vec<&ManifestItem>, Vec<&ManifestItem>) =
        manifest.items.iter().partition(|i| i.bank.is_some());

    for item in pinned {
        let bank_id = item.bank.expect("partitioned on bank.is_some()");
        if bank_id >= total_banks {
            return Err(BankError::BankOutOfRange {
                item: item.name.clone(),
                bank: bank_id,
                total_banks,
            });
        }
        let bank = &mut banks[bank_id as usize];
        let available = bank.capacity - bank.used;
        if item.size > available {
            return Err(BankError::Overflow {
                item: item.name.clone(),
                bank: Some(bank_id),
                needed: item.size,
                available,
                over_by: item.size - available,
            });
        }
        bank.used += item.size;
        bank.items.push(PlacedItem {
            name: item.name.clone(),
            size: item.size,
        });
    }

    // First-fit decreasing: largest bankable items placed first, each into
    // the first bank (1..total_banks -- bank 0 is reserved for pinned/fixed
    // content) with enough remaining room.
    let mut bankable_sorted = bankable;
    bankable_sorted.sort_by_key(|item| std::cmp::Reverse(item.size));

    for item in bankable_sorted {
        let placed = banks[1..]
            .iter_mut()
            .find(|bank| bank.capacity - bank.used >= item.size);
        match placed {
            Some(bank) => {
                bank.used += item.size;
                bank.items.push(PlacedItem {
                    name: item.name.clone(),
                    size: item.size,
                });
            }
            None => {
                let best_available = banks[1..]
                    .iter()
                    .map(|bank| bank.capacity - bank.used)
                    .max()
                    .unwrap_or(0);
                return Err(BankError::Overflow {
                    item: item.name.clone(),
                    bank: None,
                    needed: item.size,
                    available: best_available,
                    over_by: item.size - best_available,
                });
            }
        }
    }

    Ok(BankPlan {
        mbc: mbc.as_str().to_string(),
        total_banks,
        banks,
    })
}

pub fn write_bank_plan_json(path: &Path, plan: &BankPlan) -> io::Result<()> {
    let json = serde_json::to_string_pretty(plan).expect("BankPlan serializes infallibly");
    fs::write(path, json)
}

/// RGBDS linker-script fragment: `SECTION "name", ROM0` for bank 0 (the
/// fixed/always-mapped bank), `SECTION "name", ROMX, BANK[n]` for n >= 1.
pub fn render_linker_script(plan: &BankPlan) -> String {
    let mut out = String::new();
    for bank in &plan.banks {
        for item in &bank.items {
            if bank.id == 0 {
                out.push_str(&format!(
                    "SECTION \"{}\", ROM0\n; {} bytes\n\n",
                    item.name, item.size
                ));
            } else {
                out.push_str(&format!(
                    "SECTION \"{}\", ROMX, BANK[{}]\n; {} bytes\n\n",
                    item.name, bank.id, item.size
                ));
            }
        }
    }
    out
}

pub fn write_linker_script(path: &Path, plan: &BankPlan) -> io::Result<()> {
    fs::write(path, render_linker_script(plan))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(cart_size: u32, items: Vec<ManifestItem>) -> Manifest {
        Manifest {
            mbc: "MBC5".to_string(),
            cart_size,
            items,
        }
    }

    fn item(name: &str, size: u32, bank: Option<u16>) -> ManifestItem {
        ManifestItem {
            name: name.to_string(),
            size,
            bank,
        }
    }

    #[test]
    fn fits_cleanly_into_available_banks() {
        // cart = 3 banks (0, 1, 2). Bank 0 reserved for pinned/fixed only.
        let m = manifest(
            BANK_SIZE * 3,
            vec![
                item("big_sprite_sheet", 10_000, None),
                item("small_font", 4_000, None),
                item("tiny_icon", 1_000, None),
            ],
        );
        let plan = allocate(&m).unwrap();
        assert_eq!(plan.total_banks, 3);
        // Bank 0 untouched (no pinned items).
        assert_eq!(plan.banks[0].items.len(), 0);
        // First-fit-decreasing: big_sprite_sheet (10000) -> bank 1 first.
        assert_eq!(plan.banks[1].items[0].name, "big_sprite_sheet");
        // small_font (4000) still fits alongside it in bank 1 (10000+4000=14000 <= 16384).
        assert_eq!(plan.banks[1].items[1].name, "small_font");
        // tiny_icon (1000) also fits in bank 1's remaining 2384 bytes.
        assert_eq!(plan.banks[1].items[2].name, "tiny_icon");
        assert_eq!(plan.banks[1].used, 15_000);
        assert_eq!(plan.banks[2].items.len(), 0);
    }

    #[test]
    fn overflow_names_the_item_and_byte_deficit() {
        // cart = 2 banks (0, 1): only bank 1 is bankable, 16384 bytes.
        let m = manifest(
            BANK_SIZE * 2,
            vec![
                item("level_data", 15_000, None),
                item("music_bank", 10_000, None),
            ],
        );
        let err = allocate(&m).unwrap_err();
        match err {
            BankError::Overflow {
                item,
                bank: None,
                needed,
                available,
                over_by,
            } => {
                assert_eq!(item, "music_bank");
                assert_eq!(needed, 10_000);
                assert_eq!(available, BANK_SIZE - 15_000);
                assert_eq!(over_by, 10_000 - (BANK_SIZE - 15_000));
            }
            other => panic!("expected Overflow, got {other:?}"),
        }
    }

    #[test]
    fn explicit_pin_is_honoured() {
        let m = manifest(
            BANK_SIZE * 3,
            vec![
                item("header_stub", 512, Some(0)),
                item("level3_map", 8_000, Some(2)),
                item("filler", 1_000, None),
            ],
        );
        let plan = allocate(&m).unwrap();
        assert_eq!(plan.banks[0].items[0].name, "header_stub");
        assert_eq!(plan.banks[0].used, 512);
        assert_eq!(plan.banks[2].items[0].name, "level3_map");
        assert_eq!(plan.banks[2].used, 8_000);
        // The bankable item must NOT land in bank 2 just because it has
        // room too -- first-fit starts at bank 1.
        assert_eq!(plan.banks[1].items[0].name, "filler");
    }

    #[test]
    fn pinned_item_overflowing_its_bank_is_a_hard_error() {
        let m = manifest(
            BANK_SIZE * 2,
            vec![item("too_big_for_bank_0", BANK_SIZE + 1, Some(0))],
        );
        let err = allocate(&m).unwrap_err();
        match err {
            BankError::Overflow {
                item,
                bank: Some(0),
                over_by,
                ..
            } => {
                assert_eq!(item, "too_big_for_bank_0");
                assert_eq!(over_by, 1);
            }
            other => panic!("expected pinned Overflow, got {other:?}"),
        }
    }

    #[test]
    fn pin_to_unknown_bank_is_rejected() {
        let m = manifest(BANK_SIZE * 2, vec![item("ghost", 100, Some(5))]);
        let err = allocate(&m).unwrap_err();
        assert!(matches!(
            err,
            BankError::BankOutOfRange {
                bank: 5,
                total_banks: 2,
                ..
            }
        ));
    }

    #[test]
    fn rejects_misaligned_cart_size() {
        let m = manifest(BANK_SIZE + 1, vec![]);
        let err = allocate(&m).unwrap_err();
        assert!(matches!(err, BankError::CartSizeNotBankAligned { .. }));
    }

    #[test]
    fn rejects_unknown_mbc() {
        let mut m = manifest(BANK_SIZE, vec![]);
        m.mbc = "MBC9000".to_string();
        let err = allocate(&m).unwrap_err();
        assert!(matches!(err, BankError::UnknownMbc { .. }));
    }

    #[test]
    fn linker_script_uses_rom0_for_bank_zero_and_romx_bank_for_others() {
        let m = manifest(
            BANK_SIZE * 2,
            vec![
                item("fixed_code", 100, Some(0)),
                item("level_data", 100, Some(1)),
            ],
        );
        let plan = allocate(&m).unwrap();
        let script = render_linker_script(&plan);
        assert!(script.contains("SECTION \"fixed_code\", ROM0"));
        assert!(script.contains("SECTION \"level_data\", ROMX, BANK[1]"));
    }
}
