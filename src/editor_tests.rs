use crate::editor::{Editor, MAX_CHARS, wrap};

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn rows(items: &[&str], cursor: (usize, usize), width: usize) -> Vec<String> {
    wrap(&lines(items), cursor, width)
        .rows
        .into_iter()
        .map(|r| r.text)
        .collect()
}

fn cursor_in(items: &[&str], cursor: (usize, usize), width: usize) -> (usize, usize) {
    wrap(&lines(items), cursor, width).cursor
}

fn typed(text: &str) -> Editor {
    let mut editor = Editor::new();
    for c in text.chars() {
        if c == '\n' {
            editor.newline();
        } else {
            editor.insert_char(c);
        }
    }
    editor
}

// --- the buffer ---------------------------------------------------------------------------------

#[test]
fn a_new_editor_is_one_empty_line() {
    let editor = Editor::new();
    assert_eq!(editor.lines(), ["".to_string()]);
    assert_eq!(editor.text(), "");
    assert_eq!(editor.cursor(), (0, 0));
    assert!(editor.is_empty());
}

#[test]
fn text_joins_the_lines_with_newlines() {
    assert_eq!(Editor::from_text("a\n\nb").text(), "a\n\nb");
}

#[test]
fn from_text_splits_lines_and_puts_the_cursor_at_the_end() {
    let editor = Editor::from_text("ab\ncde");
    assert_eq!(editor.lines(), lines(&["ab", "cde"]).as_slice());
    assert_eq!(editor.cursor(), (1, 3));
}

#[test]
fn from_text_keeps_a_trailing_empty_line() {
    assert_eq!(
        Editor::from_text("a\n").lines(),
        lines(&["a", ""]).as_slice()
    );
}

#[test]
fn char_count_counts_characters_and_line_breaks() {
    assert_eq!(Editor::from_text("a\nbc").char_count(), 4);
    assert_eq!(Editor::from_text("日本").char_count(), 2);
}

#[test]
fn an_editor_with_text_is_not_empty() {
    assert!(!Editor::from_text("a").is_empty());
}

// --- inserting ----------------------------------------------------------------------------------

#[test]
fn a_typed_character_goes_at_the_cursor() {
    let mut editor = Editor::from_text_at("ac", 0, 1);
    assert!(editor.insert_char('b'));
    assert_eq!(editor.text(), "abc");
    assert_eq!(editor.cursor(), (0, 2));
}

#[test]
fn a_wide_character_moves_the_cursor_one_grapheme() {
    let mut editor = Editor::new();
    editor.insert_char('日');
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn a_combining_mark_joins_the_grapheme_before_it() {
    let mut editor = Editor::new();
    editor.insert_char('e');
    editor.insert_char('\u{301}');
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn control_and_bidi_characters_are_not_inserted_but_text_is() {
    let mut editor = Editor::new();
    assert!(!editor.insert_char('\u{7}'));
    assert!(!editor.insert_char('\u{202e}'));
    assert!(!editor.insert_char('\n'));
    assert!(editor.insert_char('a'));
    assert_eq!(editor.text(), "a");
}

#[test]
fn newline_splits_the_line_at_the_cursor() {
    let mut editor = Editor::from_text_at("abcd", 0, 2);
    editor.newline();
    assert_eq!(editor.lines(), lines(&["ab", "cd"]).as_slice());
    assert_eq!(editor.cursor(), (1, 0));
}

#[test]
fn newline_at_the_end_opens_an_empty_line() {
    let mut editor = Editor::from_text("ab");
    editor.newline();
    assert_eq!(editor.lines(), lines(&["ab", ""]).as_slice());
    assert_eq!(editor.cursor(), (1, 0));
}

#[test]
fn tab_inserts_four_spaces_wherever_the_cursor_is() {
    let mut editor = Editor::from_text_at("abc", 0, 1);
    editor.insert_tab();
    assert_eq!(editor.text(), "a    bc");
    assert_eq!(editor.cursor(), (0, 5));
}

// --- deleting -----------------------------------------------------------------------------------

#[test]
fn backspace_removes_the_character_before_the_cursor() {
    let mut editor = Editor::from_text("abc");
    editor.backspace();
    assert_eq!(editor.text(), "ab");
    assert_eq!(editor.cursor(), (0, 2));
}

#[test]
fn backspace_removes_a_whole_grapheme_with_its_combining_mark() {
    let mut editor = Editor::from_text("xe\u{301}");
    editor.backspace();
    assert_eq!(editor.text(), "x");
}

#[test]
fn backspace_removes_a_whole_zwj_emoji() {
    let mut editor = Editor::from_text("a👨\u{200d}👩\u{200d}👧");
    editor.backspace();
    assert_eq!(editor.text(), "a");
}

#[test]
fn backspace_at_a_line_start_joins_it_to_the_line_above() {
    let mut editor = Editor::from_text_at("ab\ncd", 1, 0);
    editor.backspace();
    assert_eq!(editor.lines(), lines(&["abcd"]).as_slice());
    assert_eq!(editor.cursor(), (0, 2));
}

#[test]
fn backspace_at_the_start_of_the_text_does_nothing() {
    let mut editor = Editor::from_text_at("ab", 0, 0);
    editor.backspace();
    assert_eq!(editor.text(), "ab");
}

#[test]
fn delete_removes_the_character_at_the_cursor() {
    let mut editor = Editor::from_text_at("abc", 0, 1);
    editor.delete();
    assert_eq!(editor.text(), "ac");
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn delete_removes_a_whole_grapheme() {
    let mut editor = Editor::from_text_at("e\u{301}x", 0, 0);
    editor.delete();
    assert_eq!(editor.text(), "x");
}

#[test]
fn delete_at_a_line_end_joins_the_next_line() {
    let mut editor = Editor::from_text_at("ab\ncd", 0, 2);
    editor.delete();
    assert_eq!(editor.lines(), lines(&["abcd"]).as_slice());
    assert_eq!(editor.cursor(), (0, 2));
}

#[test]
fn delete_at_the_end_of_the_text_does_nothing() {
    let mut editor = Editor::from_text("ab");
    editor.delete();
    assert_eq!(editor.text(), "ab");
}

#[test]
fn ctrl_u_deletes_back_to_the_start_of_the_line() {
    let mut editor = Editor::from_text_at("hello world", 0, 5);
    editor.kill_to_line_start();
    assert_eq!(editor.text(), " world");
    assert_eq!(editor.cursor(), (0, 0));
}

#[test]
fn ctrl_u_leaves_the_other_lines_alone() {
    let mut editor = Editor::from_text_at("one\ntwo\nthree", 1, 2);
    editor.kill_to_line_start();
    assert_eq!(editor.text(), "one\no\nthree");
}

#[test]
fn ctrl_u_at_a_line_start_does_nothing() {
    let mut editor = Editor::from_text_at("one\ntwo", 1, 0);
    editor.kill_to_line_start();
    assert_eq!(editor.text(), "one\ntwo");
}

#[test]
fn ctrl_w_deletes_the_word_before_the_cursor() {
    let mut editor = Editor::from_text("foo bar");
    editor.delete_word_back();
    assert_eq!(editor.text(), "foo ");
}

#[test]
fn ctrl_w_skips_trailing_spaces_first() {
    let mut editor = Editor::from_text("foo bar  ");
    editor.delete_word_back();
    assert_eq!(editor.text(), "foo ");
}

#[test]
fn ctrl_w_in_the_middle_of_a_word_deletes_to_the_word_start() {
    let mut editor = Editor::from_text_at("foo bar", 0, 6);
    editor.delete_word_back();
    assert_eq!(editor.text(), "foo r");
    assert_eq!(editor.cursor(), (0, 4));
}

#[test]
fn ctrl_w_at_a_line_start_does_nothing() {
    let mut editor = Editor::from_text_at("ab\ncd", 1, 0);
    editor.delete_word_back();
    assert_eq!(editor.text(), "ab\ncd");
}

// --- moving -------------------------------------------------------------------------------------

#[test]
fn left_and_right_move_one_grapheme() {
    let mut editor = Editor::from_text("e\u{301}x");
    editor.left();
    assert_eq!(editor.cursor(), (0, 1));
    editor.left();
    assert_eq!(editor.cursor(), (0, 0));
    editor.right();
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn left_at_a_line_start_goes_to_the_end_of_the_line_above() {
    let mut editor = Editor::from_text_at("ab\ncd", 1, 0);
    editor.left();
    assert_eq!(editor.cursor(), (0, 2));
}

#[test]
fn right_at_a_line_end_goes_to_the_start_of_the_line_below() {
    let mut editor = Editor::from_text_at("ab\ncd", 0, 2);
    editor.right();
    assert_eq!(editor.cursor(), (1, 0));
}

#[test]
fn left_and_right_stop_at_the_ends_of_the_text() {
    let mut editor = Editor::from_text_at("a", 0, 0);
    editor.left();
    assert_eq!(editor.cursor(), (0, 0));
    editor.right();
    editor.right();
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn up_and_down_move_between_lines_keeping_the_column() {
    let mut editor = Editor::from_text_at("abcd\nefgh\nijkl", 1, 2);
    editor.up(80);
    assert_eq!(editor.cursor(), (0, 2));
    editor.down(80);
    editor.down(80);
    assert_eq!(editor.cursor(), (2, 2));
}

#[test]
fn up_onto_a_shorter_line_lands_at_its_end() {
    let mut editor = Editor::from_text_at("ab\ncdefg", 1, 4);
    editor.up(80);
    assert_eq!(editor.cursor(), (0, 2));
}

#[test]
fn down_onto_a_shorter_line_lands_at_its_end() {
    let mut editor = Editor::from_text_at("abcdef\ngh", 0, 5);
    editor.down(80);
    assert_eq!(editor.cursor(), (1, 2));
}

#[test]
fn up_on_the_first_row_and_down_on_the_last_do_nothing() {
    let mut editor = Editor::from_text_at("ab\ncd", 0, 1);
    editor.up(80);
    assert_eq!(editor.cursor(), (0, 1));
    let mut editor = Editor::from_text_at("ab\ncd", 1, 1);
    editor.down(80);
    assert_eq!(editor.cursor(), (1, 1));
}

#[test]
fn up_and_down_follow_the_wrapped_rows_of_one_line() {
    let mut editor = Editor::from_text_at("abcdefghij", 0, 9);
    editor.up(4);
    assert_eq!(editor.cursor(), (0, 5));
    editor.up(4);
    assert_eq!(editor.cursor(), (0, 1));
    editor.down(4);
    assert_eq!(editor.cursor(), (0, 5));
    editor.down(4);
    assert_eq!(editor.cursor(), (0, 9));
}

#[test]
fn down_from_a_wrapped_row_onto_the_next_line_keeps_the_cell_column() {
    let mut editor = Editor::from_text_at("abcdefgh\nij", 0, 7);
    editor.down(4);
    assert_eq!(editor.cursor(), (1, 2));
}

#[test]
fn up_over_wide_characters_keeps_the_cell_column() {
    let mut editor = Editor::from_text_at("あいう", 0, 3);
    editor.up(4);
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn home_and_end_move_within_the_line() {
    let mut editor = Editor::from_text_at("ab\ncdef", 1, 2);
    editor.home();
    assert_eq!(editor.cursor(), (1, 0));
    editor.end();
    assert_eq!(editor.cursor(), (1, 4));
}

#[test]
fn home_and_end_ignore_how_the_line_is_wrapped() {
    let mut editor = Editor::from_text_at("abcdefghij", 0, 5);
    editor.home();
    assert_eq!(editor.cursor(), (0, 0));
    editor.end();
    assert_eq!(editor.cursor(), (0, 10));
}

// --- the 8000-character cap ---------------------------------------------------------------------

#[test]
fn the_cap_is_8000_characters() {
    assert_eq!(MAX_CHARS, 8000);
}

#[test]
fn a_character_past_the_cap_is_dropped() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS));
    assert!(!editor.insert_char('b'));
    assert_eq!(editor.char_count(), MAX_CHARS);
}

#[test]
fn the_last_character_that_fits_is_inserted() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS - 1));
    assert!(editor.insert_char('b'));
    assert_eq!(editor.char_count(), MAX_CHARS);
}

#[test]
fn a_newline_counts_toward_the_cap() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS));
    editor.newline();
    assert_eq!(editor.lines().len(), 1);
}

#[test]
fn a_tab_that_does_not_fit_is_dropped_whole() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS - 3));
    editor.insert_tab();
    assert_eq!(editor.char_count(), MAX_CHARS - 3);
}

#[test]
fn deleting_makes_room_again() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS));
    editor.backspace();
    assert!(editor.insert_char('b'));
}

// --- pasting ------------------------------------------------------------------------------------

#[test]
fn a_paste_keeps_its_line_breaks() {
    let mut editor = Editor::new();
    assert!(!editor.paste("a\nb\n\nc"));
    assert_eq!(editor.lines(), lines(&["a", "b", "", "c"]).as_slice());
    assert_eq!(editor.cursor(), (3, 1));
}

#[test]
fn a_paste_turns_crlf_and_cr_into_newlines() {
    let mut editor = Editor::new();
    editor.paste("a\r\nb\rc");
    assert_eq!(editor.text(), "a\nb\nc");
}

#[test]
fn a_paste_turns_tabs_into_four_spaces() {
    let mut editor = Editor::new();
    editor.paste("a\tb");
    assert_eq!(editor.text(), "a    b");
}

#[test]
fn a_paste_drops_control_and_bidi_characters() {
    let mut editor = Editor::new();
    editor.paste("a\u{7}b\u{202e}c\u{200e}d\u{1b}[31m");
    assert_eq!(editor.text(), "abcd[31m");
}

#[test]
fn a_paste_lands_at_the_cursor_in_one_piece() {
    let mut editor = Editor::from_text_at("ab", 0, 1);
    editor.paste("X\nY");
    assert_eq!(editor.lines(), lines(&["aX", "Yb"]).as_slice());
    assert_eq!(editor.cursor(), (1, 1));
}

#[test]
fn a_paste_of_exactly_the_cap_is_not_truncated() {
    let mut editor = Editor::new();
    assert!(!editor.paste(&"a".repeat(MAX_CHARS)));
    assert_eq!(editor.char_count(), MAX_CHARS);
}

#[test]
fn a_paste_past_the_cap_is_cut_and_says_so() {
    let mut editor = Editor::new();
    assert!(editor.paste(&"a".repeat(MAX_CHARS + 1)));
    assert_eq!(editor.char_count(), MAX_CHARS);
}

#[test]
fn a_paste_into_a_nearly_full_editor_fills_it_and_says_so() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS - 5));
    assert!(editor.paste(&"b".repeat(10)));
    assert_eq!(editor.char_count(), MAX_CHARS);
    assert!(editor.text().ends_with("bbbbb"));
}

#[test]
fn characters_a_paste_drops_do_not_count_toward_the_cap() {
    let mut editor = Editor::new();
    let paste = format!("{}{}", "a".repeat(MAX_CHARS), "\u{200e}".repeat(100));
    assert!(!editor.paste(&paste));
    assert_eq!(editor.char_count(), MAX_CHARS);
}

#[test]
fn a_tab_in_a_paste_is_cut_between_its_spaces_at_the_cap() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS - 3));
    assert!(editor.paste("\t"));
    assert_eq!(editor.char_count(), MAX_CHARS);
}

#[test]
fn typed_text_round_trips_through_text() {
    assert_eq!(typed("a\nb").text(), "a\nb");
}

// --- wrapping -----------------------------------------------------------------------------------

#[test]
fn a_short_line_is_one_row() {
    assert_eq!(rows(&["abc"], (0, 3), 10), ["abc"]);
    assert_eq!(cursor_in(&["abc"], (0, 3), 10), (0, 3));
}

#[test]
fn an_empty_text_is_one_empty_row_with_the_cursor_at_the_origin() {
    assert_eq!(rows(&[""], (0, 0), 10), [""]);
    assert_eq!(cursor_in(&[""], (0, 0), 10), (0, 0));
}

#[test]
fn a_long_line_breaks_at_the_width() {
    assert_eq!(rows(&["abcdef"], (0, 0), 4), ["abcd", "ef"]);
}

#[test]
fn every_line_starts_a_row() {
    assert_eq!(rows(&["ab", "", "cd"], (0, 0), 10), ["ab", "", "cd"]);
    assert_eq!(cursor_in(&["ab", "", "cd"], (2, 1), 10), (2, 1));
}

#[test]
fn the_cursor_follows_the_text_onto_the_wrapped_row() {
    assert_eq!(cursor_in(&["abcdef"], (0, 5), 4), (1, 1));
    assert_eq!(cursor_in(&["abcdef"], (0, 2), 4), (0, 2));
}

#[test]
fn a_wide_character_that_does_not_fit_moves_to_the_next_row() {
    assert_eq!(rows(&["abあ"], (0, 3), 3), ["ab", "あ"]);
    assert_eq!(cursor_in(&["abあ"], (0, 3), 3), (1, 2));
}

#[test]
fn the_cursor_before_a_moved_wide_character_sits_on_the_next_row() {
    assert_eq!(cursor_in(&["abあc"], (0, 2), 3), (1, 0));
    assert_eq!(rows(&["abあc"], (0, 0), 3), ["ab", "あc"]);
}

#[test]
fn a_wide_character_that_fits_stays() {
    assert_eq!(rows(&["aあ"], (0, 2), 4), ["aあ"]);
    assert_eq!(cursor_in(&["aあ"], (0, 2), 4), (0, 3));
}

#[test]
fn a_row_ending_in_a_wide_character_at_the_width_puts_the_cursor_on_a_new_row() {
    assert_eq!(rows(&["aあ"], (0, 2), 3), ["aあ", ""]);
    assert_eq!(cursor_in(&["aあ"], (0, 2), 3), (1, 0));
    assert_eq!(rows(&["あい"], (0, 2), 4), ["あい", ""]);
    assert_eq!(cursor_in(&["あい"], (0, 2), 4), (1, 0));
}

#[test]
fn a_row_that_ends_exactly_at_the_width_puts_the_cursor_on_a_new_row() {
    assert_eq!(rows(&["abcd"], (0, 4), 4), ["abcd", ""]);
    assert_eq!(cursor_in(&["abcd"], (0, 4), 4), (1, 0));
}

#[test]
fn a_full_row_with_the_cursor_elsewhere_adds_no_row() {
    assert_eq!(rows(&["abcd"], (0, 2), 4), ["abcd"]);
}

#[test]
fn a_grapheme_with_a_combining_mark_is_never_split() {
    assert_eq!(
        rows(&["e\u{301}e\u{301}"], (0, 0), 1),
        ["e\u{301}", "e\u{301}"]
    );
}

#[test]
fn a_character_wider_than_the_whole_width_gets_a_row_of_its_own() {
    assert_eq!(rows(&["あa"], (0, 0), 1), ["あ", "a"]);
}

#[test]
fn rows_of_later_lines_come_after_the_wrapped_rows_of_earlier_ones() {
    assert_eq!(rows(&["abcdef", "gh"], (1, 2), 4), ["abcd", "ef", "gh"]);
    assert_eq!(cursor_in(&["abcdef", "gh"], (1, 2), 4), (2, 2));
}

#[test]
fn a_paste_is_not_cut_inside_a_zwj_sequence() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS - 3));
    assert!(editor.paste("👨\u{200d}👩\u{200d}👧"));
    assert_eq!(editor.char_count(), MAX_CHARS - 3);
}

#[test]
fn a_paste_is_not_cut_between_a_letter_and_its_combining_mark() {
    let mut editor = Editor::from_text(&"a".repeat(MAX_CHARS - 1));
    assert!(editor.paste("e\u{301}"));
    assert_eq!(editor.char_count(), MAX_CHARS - 1);
}

#[test]
fn a_zwj_emoji_is_two_cells_wide() {
    let family = "👨\u{200d}👩\u{200d}👧";
    let text = format!("{family}a");
    assert_eq!(rows(&[&text], (0, 1), 3), [text.as_str()]);
    assert_eq!(cursor_in(&[&text], (0, 1), 3), (0, 2));
}

#[test]
fn a_width_of_zero_wraps_like_a_width_of_one() {
    assert_eq!(rows(&["abc"], (0, 3), 0), rows(&["abc"], (0, 3), 1));
    assert_eq!(
        cursor_in(&["abc"], (0, 3), 0),
        cursor_in(&["abc"], (0, 3), 1)
    );
    assert_eq!(rows(&["abc"], (0, 3), 1), ["a", "b", "c", ""]);
}

#[test]
fn up_and_down_with_a_width_of_zero_move_like_a_width_of_one() {
    let mut editor = Editor::from_text_at("abc", 0, 2);
    editor.up(0);
    assert_eq!(editor.cursor(), (0, 1));
    editor.down(0);
    assert_eq!(editor.cursor(), (0, 2));
}

#[test]
fn up_from_the_empty_row_after_a_full_row_goes_to_the_row_above() {
    let mut editor = Editor::from_text_at("abcdefgh", 0, 8);
    editor.up(4);
    assert_eq!(editor.cursor(), (0, 4));
}

#[test]
fn down_into_the_middle_of_a_wide_character_lands_on_its_left_edge() {
    let mut editor = Editor::from_text_at("abc\nあい", 0, 3);
    editor.down(80);
    assert_eq!(editor.cursor(), (1, 1));
}
