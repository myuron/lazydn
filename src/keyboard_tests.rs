use crate::keyboard::KeyboardMode;

#[test]
fn an_enhanced_terminal_is_told_to_submit_with_ctrl_enter() {
    assert_eq!(KeyboardMode { enhanced: true }.submit_hint(), "Ctrl+Enter");
}

#[test]
fn a_plain_terminal_is_told_to_submit_with_ctrl_s() {
    assert_eq!(KeyboardMode { enhanced: false }.submit_hint(), "Ctrl+S");
}

#[test]
fn nothing_is_assumed_before_the_terminal_is_asked() {
    assert!(!KeyboardMode::default().enhanced);
}
