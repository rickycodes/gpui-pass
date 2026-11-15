use std::ops::Range;
use zeroize::Zeroizing;

pub const MAX_EDIT_BYTES: usize = 16_384;
pub const MAX_EDIT_LINES: usize = 512;
pub const EDIT_LIMIT_MESSAGE: &str =
    "Editor limit: 16 KiB and 512 lines. Your text was not changed.";

pub fn supported(text: &str) -> bool {
    text.len() <= MAX_EDIT_BYTES
        && text.bytes().filter(|byte| *byte == b'\n').count() < MAX_EDIT_LINES
}

/// Build a replacement separately so old secret storage is wiped rather than reallocated.
pub fn replace(current: &str, range: Range<usize>, inserted: &str) -> Option<Zeroizing<String>> {
    if range.start > range.end
        || !current.is_char_boundary(range.start)
        || !current.is_char_boundary(range.end)
    {
        return None;
    }
    let length = current
        .len()
        .checked_sub(range.len())?
        .checked_add(inserted.len())?;
    if length > MAX_EDIT_BYTES {
        return None;
    }
    let mut next = Zeroizing::new(String::with_capacity(length));
    next.push_str(&current[..range.start]);
    next.push_str(inserted);
    next.push_str(&current[range.end..]);
    supported(&next).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_tabs_line_endings_and_unicode_exactly() {
        let input = "\tlabel: café\r\n密码\n";
        assert_eq!(replace("old", 0..3, input).unwrap().as_str(), input);
    }
    #[test]
    fn refuses_oversized_or_newline_heavy_input_without_truncation() {
        let current = "original";
        assert!(replace(current, 0..current.len(), &"x".repeat(MAX_EDIT_BYTES + 1)).is_none());
        assert!(replace(current, 0..current.len(), &"\n".repeat(MAX_EDIT_LINES)).is_none());
        assert!(replace("é", 1..2, "x").is_none());
        assert_eq!(current, "original");
    }
}
