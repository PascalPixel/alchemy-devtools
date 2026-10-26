//! lz: the two decoders for the ROM's packed resource streams, which `show`
//! uses to unpack overlays. Alchemy's own C encoders (`tools/`) write these
//! streams; this side only reads them.

/// Reads a stream's bits from the least significant end, sixteen at a time.
struct LsbBits<'a> {
    data: &'a [u8],
    cursor: usize,
    end: usize,
    value: u32,
    count: u32,
}

impl<'a> LsbBits<'a> {
    fn new(data: &'a [u8], cursor: usize, end: usize) -> Result<Self, String> {
        let mut bits = LsbBits {
            data,
            cursor,
            end,
            value: 0,
            count: 0,
        };
        if cursor & 1 != 0 {
            bits.value = u32::from(bits.byte()?);
            bits.count = 8;
        }
        bits.fill()?;
        Ok(bits)
    }

    fn need(&self, size: usize) -> Result<(), String> {
        if self.cursor + size > self.end {
            return Err("compressed input ended before terminator".into());
        }
        Ok(())
    }

    fn byte(&mut self) -> Result<u8, String> {
        self.need(1)?;
        let value = self.data[self.cursor];
        self.cursor += 1;
        Ok(value)
    }

    fn fill(&mut self) -> Result<(), String> {
        self.need(2)?;
        let word = u32::from(self.data[self.cursor]) | u32::from(self.data[self.cursor + 1]) << 8;
        self.value |= word.wrapping_shl(self.count);
        self.cursor += 2;
        self.count += 16;
        Ok(())
    }

    fn get(&mut self, count: u32) -> Result<u32, String> {
        while self.count < count {
            self.fill()?;
        }
        let mask = (1u32 << count) - 1;
        let value = self.value & mask;
        self.value >>= count;
        self.count -= count;
        Ok(value)
    }
}

fn bit_length(value: u32) -> u32 {
    if value == 0 {
        return 0;
    }
    32 - value.leading_zeros()
}

fn append_copy(
    output: &mut Vec<u8>,
    distance: u32,
    length: u32,
    maximum: u64,
) -> Result<(), String> {
    if distance < 1 || u64::from(distance) > output.len() as u64 {
        return Err(format!(
            "invalid back-reference distance at output offset 0x{:x}",
            output.len()
        ));
    }
    if length < 1 || output.len() as u64 + u64::from(length) > maximum {
        return Err("decoded output crossed configured bound".into());
    }
    for _ in 0..length {
        output.push(output[output.len() - distance as usize]);
    }
    Ok(())
}

/// A copy's length; `None` is the stream's terminator.
fn decode_length(bits: &mut LsbBits) -> Result<Option<u32>, String> {
    if bits.get(1)? == 0 {
        return Ok(Some(2));
    }
    if bits.get(1)? == 0 {
        return Ok(Some(3));
    }
    if bits.get(1)? == 0 {
        return Ok(Some(4));
    }
    if bits.get(1)? == 0 {
        return Ok(Some(5));
    }
    if bits.get(1)? == 0 {
        return Ok(Some(if bits.get(1)? != 0 { 7 } else { 6 }));
    }
    let short = bits.get(2)?;
    if short != 0 {
        return Ok(Some(short + 7));
    }
    let long = bits.get(7)?;
    Ok(if long == 0 { None } else { Some(long + 10) })
}

/// Unpacks a kind-zero (general) stream starting at its kind byte, returning
/// the output and the cursor after the last bit word read.
pub fn decode_general(
    data: &[u8],
    start: usize,
    end: usize,
    maximum: u64,
) -> Result<(Vec<u8>, usize), String> {
    if start >= end || data[start] != 0 {
        return Err("general stream is missing its kind-zero header".into());
    }
    let mut bits = LsbBits::new(data, start + 1, end)?;
    let mut output: Vec<u8> = Vec::new();
    loop {
        if bits.get(1)? != 0 {
            if output.len() as u64 >= maximum {
                return Err("decoded output crossed configured bound".into());
            }
            output.push(bits.get(8)? as u8);
            continue;
        }
        let Some(length) = decode_length(&mut bits)? else {
            return Ok((output, bits.cursor));
        };
        let distance = if bits.get(1)? != 0 {
            bits.get(5)? + 1
        } else {
            let window = output.len() as i64 - 33;
            let width = if (0..2048).contains(&window) {
                bit_length(window as u32)
            } else {
                12
            };
            bits.get(width)? + 33
        };
        append_copy(&mut output, distance, length, maximum)?;
    }
}

/// Unpacks a kind-one (palette) stream starting after its kind byte,
/// returning the output and the cursor after its terminator.
pub fn decode_palette(
    data: &[u8],
    start: usize,
    end: usize,
    maximum: u64,
) -> Result<(Vec<u8>, usize), String> {
    let mut cursor = start;
    let mut output: Vec<u8> = Vec::new();
    macro_rules! take {
        ($size:expr) => {{
            let size: usize = $size;
            if cursor + size > end {
                return Err("compressed input ended before terminator".into());
            }
            let result = &data[cursor..cursor + size];
            cursor += size;
            result
        }};
    }
    loop {
        let flags = take!(1)[0];
        if flags == 0 {
            let block = take!(8).to_vec();
            if output.len() as u64 + 8 > maximum {
                return Err("decoded output crossed configured bound".into());
            }
            output.extend_from_slice(&block);
            continue;
        }
        for bit in (0..8).rev() {
            if flags & (1 << bit) == 0 {
                if output.len() as u64 >= maximum {
                    return Err("decoded output crossed configured bound".into());
                }
                let value = take!(1)[0];
                output.push(value);
                continue;
            }
            let pair = take!(2);
            let (first, second) = (pair[0], pair[1]);
            let distance = (u32::from(first & 0xf0) << 4) | u32::from(second);
            let mut length = u32::from(first & 0x0f);
            if length == 0 {
                if distance == 0 {
                    return Ok((output, cursor));
                }
                length = u32::from(take!(1)[0]) + 17;
            } else {
                length += 1;
            }
            append_copy(&mut output, distance, length, maximum)?;
        }
    }
}
