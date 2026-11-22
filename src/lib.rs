// The native text view retains source text. Only these ASCII prefixes may be hidden.
fn block_prefix(line: &[u8]) -> usize {
    let hashes = line.iter().take_while(|&&c| c == b'#').count();
    if (1..=6).contains(&hashes) && line.get(hashes) == Some(&b' ') {
        return hashes + 1;
    }
    if line.starts_with(b"> ") || line.starts_with(b"- ") || line.starts_with(b"* ") {
        return 2;
    }
    0
}

/// # Safety
/// The caller supplies a readable buffer of `len` bytes for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn folio_prefix(data: *const u8, len: usize) -> usize {
    if data.is_null() {
        return 0;
    }
    block_prefix(unsafe { std::slice::from_raw_parts(data, len) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_only_complete_markers() {
        for (line, expected) in [
            ("# Title", 2),
            ("###### 六", 7),
            ("####### x", 0),
            ("#tag", 0),
            ("- item", 2),
            ("* item", 2),
            ("> quote", 2),
            ("  # code", 0),
        ] {
            assert_eq!(block_prefix(line.as_bytes()), expected);
        }
    }
}
