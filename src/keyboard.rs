/// What the terminal can tell apart, decided once at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardMode {
    /// The terminal reports Ctrl+Enter apart from Enter.
    pub enhanced: bool,
}

impl KeyboardMode {
    /// The submit key named in the hint line.
    pub fn submit_hint(self) -> &'static str {
        if self.enhanced {
            "Ctrl+Enter"
        } else {
            "Ctrl+S"
        }
    }
}
