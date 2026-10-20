//! The ASCII mode of the lists: lazydn's own glyphs (rails, nodes, rules, marks and the cuts it makes) are chosen
//! where they are drawn, so that what people wrote is never rewritten.

/// `unicode` normally and `plain` in the ASCII mode.
pub fn pick(ascii: bool, unicode: &'static str, plain: &'static str) -> &'static str {
    if ascii { plain } else { unicode }
}
