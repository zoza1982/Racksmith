//! Hexadecimal digit decoding shared by the identifier parsers.

/// Returns the value of a lowercase ASCII hexadecimal digit, or `None` for any other byte.
///
/// Uppercase digits are deliberately rejected; callers that accept them lowercase first.
pub(crate) const fn lower_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::lower_digit;

    #[test]
    fn decodes_every_lowercase_digit() {
        for (expected, byte) in (0..).zip(b"0123456789abcdef".iter().copied()) {
            assert_eq!(
                lower_digit(byte),
                Some(expected),
                "digit {}",
                char::from(byte)
            );
        }
    }

    #[test]
    fn rejects_uppercase_and_non_digits() {
        for byte in [b'A', b'F', b'g', b'+', b'-', b' ', 0xff] {
            assert_eq!(lower_digit(byte), None, "byte {byte:#04x}");
        }
    }
}
