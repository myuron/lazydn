/// Control characters and the marks that reorder or break a line; none belongs in text from the user,
/// the server or the OS. Covers the bidi marks (U+061C, U+200E/F, U+202A-E, U+2066-9) and U+2028/9.
pub fn is_unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}
