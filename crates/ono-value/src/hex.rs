//! Hexadecimal, which is how every text codec here carries a byte that is not text.
//!
//! Spec §12.2 requires that undecodable bytes are never lost. Hex is the one encoding that costs
//! nothing to read, survives every format in this crate, and cannot be mistaken for a decoded
//! string.

/// Encodes bytes as lower-case hexadecimal.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    out
}

/// Decodes lower- or upper-case hexadecimal, or `None` when the text is not hexadecimal.
pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in bytes.chunks(2) {
        let high = char::from(*pair.first()?).to_digit(16)?;
        let low = char::from(*pair.get(1)?).to_digit(16)?;
        out.push(u8::try_from(high * 16 + low).ok()?);
    }
    Some(out)
}
