use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

use crate::keymap::{ComposerKey, map_composer_key};
use crate::test_support::key_event;

fn press(code: KeyCode, modifiers: KeyModifiers) -> Option<ComposerKey> {
    map_composer_key(key_event(code, modifiers, KeyEventKind::Press))
}

fn plain(code: KeyCode) -> Option<ComposerKey> {
    press(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> Option<ComposerKey> {
    press(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn kind(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Option<ComposerKey> {
    map_composer_key(key_event(code, modifiers, kind))
}

// --- text ---------------------------------------------------------------------------------------

#[test]
fn characters_are_typed() {
    assert_eq!(plain(KeyCode::Char('a')), Some(ComposerKey::Insert('a')));
    assert_eq!(plain(KeyCode::Char(' ')), Some(ComposerKey::Insert(' ')));
    assert_eq!(plain(KeyCode::Char('日')), Some(ComposerKey::Insert('日')));
}

#[test]
fn a_shifted_character_is_typed() {
    assert_eq!(
        press(KeyCode::Char('A'), KeyModifiers::SHIFT),
        Some(ComposerKey::Insert('A'))
    );
}

#[test]
fn keys_with_a_global_meaning_are_typed_as_text() {
    for c in ['j', 'k', 'q', 'g', '?', 'Q', 'c', 'r', 'L', '1', '@'] {
        assert_eq!(plain(KeyCode::Char(c)), Some(ComposerKey::Insert(c)));
    }
}

#[test]
fn alt_with_a_character_is_ignored() {
    assert_eq!(press(KeyCode::Char('a'), KeyModifiers::ALT), None);
}

#[test]
fn tab_inserts_spaces() {
    assert_eq!(plain(KeyCode::Tab), Some(ComposerKey::Tab));
}

#[test]
fn back_tab_is_ignored() {
    assert_eq!(plain(KeyCode::BackTab), None);
    assert_eq!(press(KeyCode::BackTab, KeyModifiers::SHIFT), None);
}

// --- new lines ----------------------------------------------------------------------------------

#[test]
fn enter_starts_a_new_line() {
    assert_eq!(plain(KeyCode::Enter), Some(ComposerKey::Newline));
}

#[test]
fn shift_enter_alt_enter_and_ctrl_j_start_a_new_line() {
    assert_eq!(
        press(KeyCode::Enter, KeyModifiers::SHIFT),
        Some(ComposerKey::Newline)
    );
    assert_eq!(
        press(KeyCode::Enter, KeyModifiers::ALT),
        Some(ComposerKey::Newline)
    );
    assert_eq!(ctrl('j'), Some(ComposerKey::Newline));
}

// --- submitting and leaving ---------------------------------------------------------------------

#[test]
fn ctrl_enter_and_ctrl_s_submit() {
    assert_eq!(
        press(KeyCode::Enter, KeyModifiers::CONTROL),
        Some(ComposerKey::Submit)
    );
    assert_eq!(ctrl('s'), Some(ComposerKey::Submit));
}

#[test]
fn escape_closes() {
    assert_eq!(plain(KeyCode::Esc), Some(ComposerKey::Close));
}

#[test]
fn ctrl_c_quits() {
    assert_eq!(ctrl('c'), Some(ComposerKey::Quit));
}

#[test]
fn other_ctrl_letters_are_ignored() {
    for c in ['a', 'q', 'r', 'o', 'y', 'l', 'z'] {
        assert_eq!(ctrl(c), None);
    }
}

// --- editing ------------------------------------------------------------------------------------

#[test]
fn editing_keys_map_to_their_commands() {
    assert_eq!(plain(KeyCode::Backspace), Some(ComposerKey::Backspace));
    assert_eq!(plain(KeyCode::Delete), Some(ComposerKey::Delete));
    assert_eq!(plain(KeyCode::Left), Some(ComposerKey::Left));
    assert_eq!(plain(KeyCode::Right), Some(ComposerKey::Right));
    assert_eq!(plain(KeyCode::Up), Some(ComposerKey::Up));
    assert_eq!(plain(KeyCode::Down), Some(ComposerKey::Down));
    assert_eq!(plain(KeyCode::Home), Some(ComposerKey::Home));
    assert_eq!(plain(KeyCode::End), Some(ComposerKey::End));
}

#[test]
fn ctrl_u_and_ctrl_w_delete_backwards() {
    assert_eq!(ctrl('u'), Some(ComposerKey::KillToLineStart));
    assert_eq!(ctrl('w'), Some(ComposerKey::DeleteWordBack));
}

// --- key events that are not presses ------------------------------------------------------------

#[test]
fn a_release_is_ignored() {
    for code in [KeyCode::Char('a'), KeyCode::Backspace, KeyCode::Esc] {
        assert_eq!(kind(code, KeyModifiers::NONE, KeyEventKind::Release), None);
    }
    assert_eq!(
        kind(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL,
            KeyEventKind::Release
        ),
        None
    );
}

#[test]
fn a_held_key_repeats_text_and_deletion() {
    assert_eq!(
        kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Repeat),
        Some(ComposerKey::Insert('a'))
    );
    assert_eq!(
        kind(KeyCode::Backspace, KeyModifiers::NONE, KeyEventKind::Repeat),
        Some(ComposerKey::Backspace)
    );
    assert_eq!(
        kind(KeyCode::Left, KeyModifiers::NONE, KeyEventKind::Repeat),
        Some(ComposerKey::Left)
    );
}

#[test]
fn a_held_key_does_not_repeat_submit_close_or_quit() {
    for (code, modifiers) in [
        (KeyCode::Char('s'), KeyModifiers::CONTROL),
        (KeyCode::Enter, KeyModifiers::CONTROL),
        (KeyCode::Esc, KeyModifiers::NONE),
        (KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        assert_eq!(kind(code, modifiers, KeyEventKind::Repeat), None);
    }
}

// --- AltGr ----------------------------------------------------------------------------------------

#[test]
fn a_symbol_typed_with_altgr_is_inserted() {
    for c in ['@', '{', '[', '€'] {
        assert_eq!(
            press(KeyCode::Char(c), KeyModifiers::CONTROL | KeyModifiers::ALT),
            Some(ComposerKey::Insert(c))
        );
    }
}

#[test]
fn a_letter_with_ctrl_and_alt_is_ignored() {
    for c in ['a', 'c', 's', 'u'] {
        assert_eq!(
            press(KeyCode::Char(c), KeyModifiers::CONTROL | KeyModifiers::ALT),
            None
        );
    }
}

#[test]
fn a_non_ascii_letter_typed_with_altgr_is_inserted() {
    for c in ['ł', 'ą'] {
        assert_eq!(
            press(KeyCode::Char(c), KeyModifiers::CONTROL | KeyModifiers::ALT),
            Some(ComposerKey::Insert(c))
        );
    }
}
