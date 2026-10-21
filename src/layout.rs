#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Wide,
    Medium,
    Narrow,
    TooSmall,
}

impl LayoutMode {
    pub fn from_size(width: u16, height: u16) -> Self {
        if height < 20 {
            LayoutMode::TooSmall
        } else if width >= 120 {
            LayoutMode::Wide
        } else if width >= 80 {
            LayoutMode::Medium
        } else {
            LayoutMode::Narrow
        }
    }
}
