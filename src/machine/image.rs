//! Bare-metal ARM ELF loading. PT_LOAD physical addresses preserve flash LMAs.
use super::{Error, Seed};

fn word(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let raw = bytes
        .get(
            offset
                ..offset
                    .checked_add(4)
                    .ok_or_else(|| Error::new("ELF offset overflow"))?,
        )
        .ok_or_else(|| Error::new("truncated ELF header"))?;
    Ok(u32::from_le_bytes(raw.try_into().expect("four bytes")))
}
pub(super) fn load(seed: &mut Seed, bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() < 52
        || &bytes[..7] != b"\x7fELF\x01\x01\x01"
        || word(bytes, 16)? & 0xffff != 2
        || word(bytes, 16)? >> 16 != 40
    {
        return Err(Error::new("expected a little-endian ARM ELF32 executable"));
    }
    let phoff = word(bytes, 28)? as usize;
    let descriptor = word(bytes, 40)?;
    let phsize = (descriptor >> 16) as usize;
    let count = (word(bytes, 44)? & 0xffff) as usize;
    if phsize < 32 || count == 0 {
        return Err(Error::new("ELF has no valid load headers"));
    }
    // Validate every segment before changing memory.
    let mut segments = Vec::new();
    for n in 0..count {
        let at = phoff
            .checked_add(
                n.checked_mul(phsize)
                    .ok_or_else(|| Error::new("ELF header overflow"))?,
            )
            .ok_or_else(|| Error::new("ELF header overflow"))?;
        if word(bytes, at)? != 1 {
            continue;
        }
        let offset = word(bytes, at + 4)? as usize;
        let address = word(bytes, at + 12)?;
        let size = word(bytes, at + 16)? as usize;
        let memory_size = word(bytes, at + 20)? as usize;
        if size > memory_size || !Seed::loadable(address, memory_size) {
            return Err(Error::new(format!(
                "ELF segment outside populated Seed memory at 0x{address:08x}"
            )));
        }
        let data = bytes
            .get(
                offset
                    ..offset
                        .checked_add(size)
                        .ok_or_else(|| Error::new("ELF data overflow"))?,
            )
            .ok_or_else(|| Error::new("truncated ELF segment"))?;
        segments.push((address, data));
    }
    if segments.is_empty() {
        return Err(Error::new("ELF contains no loadable segments"));
    }
    for (address, data) in segments {
        seed.load_bytes(address, data)?;
    }
    Ok(())
}
