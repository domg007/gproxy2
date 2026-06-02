//! Minimal base64 encode/decode for wasm32 cache backends.
//!
//! No external crate is needed. Both `LibsqlCache` and `UpstashCache` use
//! this module to encode/decode arbitrary byte values as base64 strings
//! for storage in the respective string-oriented backend APIs.

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        out.push(B64_CHARS[b0 >> 2] as char);
        out.push(B64_CHARS[((b0 & 0x3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(B64_CHARS[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_CHARS[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let mut table = [0u8; 256];
    for (i, &c) in B64_CHARS.iter().enumerate() {
        table[c as usize] = i as u8;
    }
    let input = input.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in input {
        buf = (buf << 6) | table[c as usize] as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let data = b"hello, world! \x00\xff\xfe";
        assert_eq!(decode(&encode(data)).unwrap(), data);
    }
}
