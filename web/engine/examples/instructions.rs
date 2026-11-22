//! Audit every disassembled firmware instruction against the decoder.
use std::{collections::BTreeMap, process::Command};
use zmu_cortex_m::{
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    Processor,
};
fn main() {
    let path = std::env::args().nth(1).expect("firmware ELF");
    let result = Command::new("arm-none-eabi-objdump")
        .args(["-d", &path])
        .output()
        .unwrap();
    assert!(result.status.success());
    let mut missing = BTreeMap::<String, (usize, String)>::new();
    let mut total = 0;
    let c = Processor::new();
    std::panic::set_hook(Box::new(|_| {}));
    for line in String::from_utf8(result.stdout).unwrap().lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 3 || !parts[0].ends_with(':') || parts[1].len() != 4 {
            continue;
        }
        let Ok(first) = u16::from_str_radix(parts[1], 16) else {
            continue;
        };
        let second = if parts[2].len() == 4 {
            u16::from_str_radix(parts[2], 16).ok()
        } else {
            None
        };
        let instruction_index = if second.is_some() { 3 } else { 2 };
        let Some(&mnemonic) = parts.get(instruction_index) else {
            continue;
        };
        if mnemonic.starts_with('.') {
            continue;
        }
        total += 1;
        let code = second.map_or(ThumbCode::Thumb16 { opcode: first }, |s| {
            ThumbCode::Thumb32 {
                opcode: (u32::from(first) << 16) | u32::from(s),
            }
        });
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.decode(code)));
        if decoded.is_err() || (matches!(decoded, Ok(Instruction::UDF { .. })) && mnemonic != "udf")
        {
            let entry = missing
                .entry(mnemonic.to_string())
                .or_insert((0, line.to_string()));
            entry.0 += 1;
        }
    }
    println!("{total} firmware instructions inspected");
    for (name, (count, example)) in &missing {
        println!("{name}: {count}; {example}");
    }
    if !missing.is_empty() {
        std::process::exit(1);
    }
}
