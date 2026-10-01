//! Minimal SM83 (LR35902) disassembler for the IDE's step-through view.
//!
//! TerminalGB's own `docs/debugging.md` documents a planned `disassemble()`
//! API (section "The disassembler -- the one real build") but it is not yet
//! implemented in the pinned embedding build this IDE depends on (no
//! `disassemble` function exists under `src/cpu/` or `src/gameboy.rs` at the
//! pinned rev) -- so this is new code, not a duplicate of an existing
//! capability. It decodes opcodes by reading through the emulator's
//! `debug_read`, matching the standard SM83 instruction encoding. Unofficial/
//! unused opcodes decode as a raw `DB $xx` byte so the view never panics on
//! input it doesn't recognize.

/// One decoded instruction: its text, and how many bytes it occupied.
pub struct Instr {
    pub address: u16,
    pub bytes: Vec<u8>,
    pub text: String,
}

const R8: [&str; 8] = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];
const R16: [&str; 4] = ["BC", "DE", "HL", "SP"];
const R16STK: [&str; 4] = ["BC", "DE", "HL", "AF"];
const COND: [&str; 4] = ["NZ", "Z", "NC", "C"];

/// Decode `count` instructions starting at `pc`, using `read` as the memory
/// accessor (the caller supplies `|a| gb.debug_read(a)` or similar).
pub fn disassemble(mut read: impl FnMut(u16) -> u8, pc: u16, count: usize) -> Vec<Instr> {
    let mut out = Vec::with_capacity(count);
    let mut addr = pc;
    for _ in 0..count {
        let instr = decode_one(&mut read, addr);
        addr = addr.wrapping_add(instr.bytes.len().max(1) as u16);
        out.push(instr);
    }
    out
}

fn decode_one(read: &mut impl FnMut(u16) -> u8, addr: u16) -> Instr {
    let op = read(addr);
    let b1 = read(addr.wrapping_add(1));
    let b2 = read(addr.wrapping_add(2));
    let imm8 = || b1;
    let imm16 = || (b1 as u16) | ((b2 as u16) << 8);
    let rel = || (b1 as i8) as i32;

    let (text, len): (String, u16) = match op {
        0x00 => ("NOP".into(), 1),
        0x10 => ("STOP".into(), 2),
        0x76 => ("HALT".into(), 1),
        0xF3 => ("DI".into(), 1),
        0xFB => ("EI".into(), 1),
        0x07 => ("RLCA".into(), 1),
        0x0F => ("RRCA".into(), 1),
        0x17 => ("RLA".into(), 1),
        0x1F => ("RRA".into(), 1),
        0x2F => ("CPL".into(), 1),
        0x3F => ("CCF".into(), 1),
        0x37 => ("SCF".into(), 1),
        0x27 => ("DAA".into(), 1),
        0xC9 => ("RET".into(), 1),
        0xD9 => ("RETI".into(), 1),
        0xE9 => ("JP HL".into(), 1),
        0xF9 => ("LD SP, HL".into(), 1),
        0xCB => decode_cb(read(addr.wrapping_add(1))),
        // JP/JR/CALL/RET conditional
        0xC3 => (format!("JP ${:04X}", imm16()), 3),
        0xC2 | 0xCA | 0xD2 | 0xDA => {
            let cc = COND[((op >> 3) & 3) as usize];
            (format!("JP {}, ${:04X}", cc, imm16()), 3)
        }
        0x18 => (format!("JR {}", fmt_rel(addr, rel())), 2),
        0x20 | 0x28 | 0x30 | 0x38 => {
            let cc = COND[((op >> 3) & 3) as usize];
            (format!("JR {}, {}", cc, fmt_rel(addr, rel())), 2)
        }
        0xCD => (format!("CALL ${:04X}", imm16()), 3),
        0xC4 | 0xCC | 0xD4 | 0xDC => {
            let cc = COND[((op >> 3) & 3) as usize];
            (format!("CALL {}, ${:04X}", cc, imm16()), 3)
        }
        0xC0 | 0xC8 | 0xD0 | 0xD8 => {
            let cc = COND[((op >> 3) & 3) as usize];
            (format!("RET {}", cc), 1)
        }
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
            (format!("RST ${:02X}", op & 0x38), 1)
        }
        // 16-bit loads / stack
        0x01 | 0x11 | 0x21 | 0x31 => {
            let r = R16[((op >> 4) & 3) as usize];
            (format!("LD {}, ${:04X}", r, imm16()), 3)
        }
        0xC1 | 0xD1 | 0xE1 | 0xF1 => {
            let r = R16STK[((op >> 4) & 3) as usize];
            (format!("POP {}", r), 1)
        }
        0xC5 | 0xD5 | 0xE5 | 0xF5 => {
            let r = R16STK[((op >> 4) & 3) as usize];
            (format!("PUSH {}", r), 1)
        }
        0x08 => (format!("LD (${:04X}), SP", imm16()), 3),
        0xF8 => (format!("LD HL, SP{:+}", rel()), 2),
        0xE8 => (format!("ADD SP, {:+}", rel()), 2),
        0x02 => ("LD (BC), A".into(), 1),
        0x12 => ("LD (DE), A".into(), 1),
        0x0A => ("LD A, (BC)".into(), 1),
        0x1A => ("LD A, (DE)".into(), 1),
        0x22 => ("LD (HL+), A".into(), 1),
        0x32 => ("LD (HL-), A".into(), 1),
        0x2A => ("LD A, (HL+)".into(), 1),
        0x3A => ("LD A, (HL-)".into(), 1),
        0xE0 => (format!("LDH (${:02X}), A", imm8()), 2),
        0xF0 => (format!("LDH A, (${:02X})", imm8()), 2),
        0xE2 => ("LD (C), A".into(), 1),
        0xF2 => ("LD A, (C)".into(), 1),
        0xEA => (format!("LD (${:04X}), A", imm16()), 3),
        0xFA => (format!("LD A, (${:04X})", imm16()), 3),
        // 8-bit LD r, d8
        _ if op & 0xC7 == 0x06 => {
            let r = R8[((op >> 3) & 7) as usize];
            (format!("LD {}, ${:02X}", r, imm8()), 2)
        }
        // 8-bit LD r, r (0x40-0x7F minus HALT already handled)
        _ if (0x40..=0x7F).contains(&op) => {
            let dst = R8[((op >> 3) & 7) as usize];
            let src = R8[(op & 7) as usize];
            (format!("LD {}, {}", dst, src), 1)
        }
        // INC/DEC r16
        _ if op & 0xC7 == 0x03 => (format!("INC {}", R16[((op >> 4) & 3) as usize]), 1),
        _ if op & 0xC7 == 0x0B => (format!("DEC {}", R16[((op >> 4) & 3) as usize]), 1),
        _ if op & 0xC7 == 0x09 => (format!("ADD HL, {}", R16[((op >> 4) & 3) as usize]), 1),
        // INC/DEC r8
        _ if op & 0xC7 == 0x04 => (format!("INC {}", R8[((op >> 3) & 7) as usize]), 1),
        _ if op & 0xC7 == 0x05 => (format!("DEC {}", R8[((op >> 3) & 7) as usize]), 1),
        // ALU A, r8 (0x80-0xBF)
        _ if (0x80..=0xBF).contains(&op) => {
            let r = R8[(op & 7) as usize];
            let (mnemonic, _) = alu_mnemonic((op >> 3) & 7);
            (format!("{} {}", mnemonic, r), 1)
        }
        // ALU A, d8 (0xC6, D6, E6, F6, ...)
        _ if op & 0xC7 == 0xC6 => {
            let (mnemonic, _) = alu_mnemonic((op >> 3) & 7);
            (format!("{} ${:02X}", mnemonic, imm8()), 2)
        }
        _ => (format!("DB ${:02X}", op), 1),
    };

    let mut bytes = Vec::with_capacity(len as usize);
    for i in 0..len {
        bytes.push(read(addr.wrapping_add(i)));
    }
    Instr { address: addr, bytes, text }
}

fn alu_mnemonic(group: u8) -> (&'static str, u8) {
    match group {
        0 => ("ADD A,", 0),
        1 => ("ADC A,", 1),
        2 => ("SUB", 2),
        3 => ("SBC A,", 3),
        4 => ("AND", 4),
        5 => ("XOR", 5),
        6 => ("OR", 6),
        _ => ("CP", 7),
    }
}

fn decode_cb(sub: u8) -> (String, u16) {
    let r = R8[(sub & 7) as usize];
    let text = match sub >> 6 {
        0 => match (sub >> 3) & 7 {
            0 => format!("RLC {r}"),
            1 => format!("RRC {r}"),
            2 => format!("RL {r}"),
            3 => format!("RR {r}"),
            4 => format!("SLA {r}"),
            5 => format!("SRA {r}"),
            6 => format!("SWAP {r}"),
            _ => format!("SRL {r}"),
        },
        1 => format!("BIT {}, {}", (sub >> 3) & 7, r),
        2 => format!("RES {}, {}", (sub >> 3) & 7, r),
        _ => format!("SET {}, {}", (sub >> 3) & 7, r),
    };
    (text, 2)
}

fn fmt_rel(addr: u16, rel: i32) -> String {
    let target = (addr as i32 + 2 + rel) as u16;
    format!("${:04X}", target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom(bytes: &[u8]) -> impl Fn(u16) -> u8 + '_ {
        move |a| bytes.get(a as usize).copied().unwrap_or(0)
    }

    #[test]
    fn decodes_nop_and_length() {
        let instrs = disassemble(rom(&[0x00, 0x00]), 0, 2);
        assert_eq!(instrs[0].text, "NOP");
        assert_eq!(instrs[0].bytes.len(), 1);
        assert_eq!(instrs[1].address, 1);
    }

    #[test]
    fn decodes_ld_immediate_16() {
        let instrs = disassemble(rom(&[0x21, 0x34, 0x12]), 0, 1);
        assert_eq!(instrs[0].text, "LD HL, $1234");
        assert_eq!(instrs[0].bytes.len(), 3);
    }

    #[test]
    fn decodes_jr_relative_forward() {
        // JR +2 from address 0 -> target 0 + 2 (instr len) + 2 = 4
        let instrs = disassemble(rom(&[0x18, 0x02]), 0, 1);
        assert_eq!(instrs[0].text, "JR $0004");
    }

    #[test]
    fn decodes_ld_r_r() {
        // 0x7D = LD A, L
        let instrs = disassemble(rom(&[0x7D]), 0, 1);
        assert_eq!(instrs[0].text, "LD A, L");
    }

    #[test]
    fn decodes_cb_bit() {
        // CB 7C = BIT 7, H
        let instrs = disassemble(rom(&[0xCB, 0x7C]), 0, 1);
        assert_eq!(instrs[0].text, "BIT 7, H");
        assert_eq!(instrs[0].bytes.len(), 2);
    }

    #[test]
    fn unknown_opcode_falls_back_to_db() {
        // 0xED is not a valid SM83 opcode
        let instrs = disassemble(rom(&[0xED]), 0, 1);
        assert_eq!(instrs[0].text, "DB $ED");
    }
}
