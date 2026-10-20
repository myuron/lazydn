use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;

use crate::highlight::HlClass;
use crate::lane::{self, to_ansi256};
use crate::model::{Lane, Rgb};

pub const PRIMARY: Color = Color::Cyan;
pub const CURSOR_BG: Color = Color::DarkGray;
const CODE_RGB: Rgb = Rgb(0x2a, 0x2e, 0x3a);
pub const CODE_BG: Color = Color::Rgb(CODE_RGB.0, CODE_RGB.1, CODE_RGB.2);
pub const DIM: Modifier = Modifier::DIM;

pub fn border_type(focused: bool) -> BorderType {
    if focused {
        BorderType::Thick
    } else {
        BorderType::Plain
    }
}

pub fn current_style() -> Style {
    Style::new().fg(PRIMARY)
}

pub fn cursor_style() -> Style {
    Style::new().bg(CURSOR_BG)
}

pub fn lane_color(lane: Lane, truecolor: bool) -> Color {
    let rgb = lane::rgb(lane);
    if truecolor {
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    } else {
        Color::Indexed(to_ansi256(rgb))
    }
}

pub fn hl_color(class: HlClass, truecolor: bool) -> Color {
    let color = hl_rgb(class);
    match (color, truecolor) {
        (Color::Rgb(r, g, b), false) => Color::Indexed(to_ansi256(Rgb(r, g, b))),
        _ => color,
    }
}

fn hl_rgb(class: HlClass) -> Color {
    match class {
        HlClass::Plain | HlClass::Unknown => Color::Reset,
        HlClass::Keyword => Color::Rgb(0xda, 0x77, 0xf2),
        HlClass::Property | HlClass::Attribute => Color::Rgb(0x74, 0xc0, 0xfc),
        HlClass::Type => Color::Rgb(0x3b, 0xc9, 0xdb),
        HlClass::Value | HlClass::Number | HlClass::Literal => Color::Rgb(0xff, 0xa9, 0x4d),
        HlClass::Comment => Color::Rgb(0x86, 0x8e, 0x96),
        HlClass::Variable => Color::Rgb(0xff, 0xd4, 0x3b),
        HlClass::Generic | HlClass::Operator => Color::Rgb(0xce, 0xd4, 0xda),
        HlClass::Addition => Color::Rgb(0x51, 0xcf, 0x66),
        HlClass::Deletion => Color::Rgb(0xff, 0x6b, 0x6b),
    }
}

pub fn code_bg(truecolor: bool) -> Color {
    if truecolor {
        CODE_BG
    } else {
        Color::Indexed(to_ansi256(CODE_RGB))
    }
}

/// Truecolor keeps the named CURSOR_BG so the card matches the nav cursor.
/// The 256-color fallback is a fixed grey because named ANSI colors are remapped by terminal themes.
pub fn cursor_bg(truecolor: bool) -> Color {
    if truecolor {
        CURSOR_BG
    } else {
        Color::Indexed(238)
    }
}
