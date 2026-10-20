use crate::body::{MAX_BODY_LEN, body_len, normalize_like_server};

// --- blank lines in the prose collapse into one (CommitBodiesTest::normalize) -------------------

#[test]
fn one_blank_line_is_kept() {
    assert_eq!(normalize_like_server("a\n\nb"), "a\n\nb");
}

#[test]
fn two_blank_lines_become_one() {
    assert_eq!(normalize_like_server("a\n\n\nb"), "a\n\nb");
}

#[test]
fn many_blank_lines_become_one() {
    assert_eq!(normalize_like_server("a\n\n\n\n\n\nb"), "a\n\nb");
}

#[test]
fn blank_lines_with_spaces_and_tabs_become_one() {
    assert_eq!(normalize_like_server("a \n\t\n  \n\nb"), "a\n\nb");
}

#[test]
fn windows_line_breaks_become_newlines() {
    assert_eq!(
        normalize_like_server("a\r\nb\r\n\r\n\r\n\r\nc"),
        "a\nb\n\nc"
    );
}

#[test]
fn standalone_carriage_returns_become_newlines() {
    assert_eq!(normalize_like_server("a\rb\r\r\rc"), "a\nb\n\nc");
}

#[test]
fn mixed_line_endings_become_newlines() {
    assert_eq!(normalize_like_server("a\r\nb\rc\n\n\rd"), "a\nb\nc\n\nd");
}

#[test]
fn blank_lines_in_a_code_block_are_kept() {
    let body = "a\n```py\ndef a():\n    pass\n\n\ndef b():\n    pass\n```\nb";
    assert_eq!(normalize_like_server(body), body);
}

#[test]
fn blank_lines_around_a_code_block_become_one() {
    assert_eq!(
        normalize_like_server("a\n\n\n\n```\ncode\n```\n\n\n\nb"),
        "a\n\n```\ncode\n```\n\nb"
    );
}

// --- what counts as a code block ----------------------------------------------------------------

#[test]
fn a_fence_that_never_closes_is_prose() {
    assert_eq!(
        normalize_like_server("a\n```\ncode\n\n\n\nb"),
        "a\n```\ncode\n\nb"
    );
}

#[test]
fn an_empty_code_block_keeps_the_blank_lines_inside() {
    assert_eq!(
        normalize_like_server("```\n\n\n\n\n```"),
        "```\n\n\n\n\n```"
    );
}

#[test]
fn a_longer_closing_fence_closes_the_block() {
    let body = "```\na\n\n\n\nb\n````\n\n\n\nc";
    assert_eq!(normalize_like_server(body), "```\na\n\n\n\nb\n````\n\nc");
}

#[test]
fn a_closing_fence_may_be_followed_by_spaces_and_tabs() {
    let body = "```\na\n\n\n\nb\n``` \t\n\n\n\nc";
    assert_eq!(normalize_like_server(body), "```\na\n\n\n\nb\n``` \t\n\nc");
}

#[test]
fn a_fence_line_with_text_after_it_does_not_close_the_block() {
    let body = "```\na\n``` x\n\n\n\nb\n```\nc";
    assert_eq!(normalize_like_server(body), body);
}

#[test]
fn an_opening_line_with_a_backtick_in_the_language_is_prose() {
    assert_eq!(
        normalize_like_server("```a`b\n\n\n\nx\n```"),
        "```a`b\n\nx\n```"
    );
}

#[test]
fn an_indented_fence_is_prose() {
    assert_eq!(
        normalize_like_server("a\n  ```\n\n\n\nx\n  ```"),
        "a\n  ```\n\nx\n  ```"
    );
}

#[test]
fn a_run_of_backticks_inside_a_line_does_not_open_a_block() {
    assert_eq!(
        normalize_like_server("x ```\n\n\n\ny\n```"),
        "x ```\n\ny\n```"
    );
}

#[test]
fn two_code_blocks_each_keep_their_blank_lines() {
    let body = "```\na\n\n\n\nb\n```\n\n\n\n```\nc\n\n\n\nd\n```";
    assert_eq!(
        normalize_like_server(body),
        "```\na\n\n\n\nb\n```\n\n```\nc\n\n\n\nd\n```"
    );
}

// --- the edges are trimmed like the server's TrimStrings ----------------------------------------

#[test]
fn surrounding_whitespace_is_trimmed() {
    assert_eq!(normalize_like_server(" \t\n a \r\n"), "a");
}

#[test]
fn unicode_spaces_are_trimmed() {
    assert_eq!(normalize_like_server("\u{a0}\u{3000}a\u{a0}"), "a");
}

#[test]
fn bom_zero_width_space_and_lrm_are_trimmed() {
    assert_eq!(
        normalize_like_server("\u{feff}\u{200b}\u{200e}a\u{200e}\u{200b}\u{feff}"),
        "a"
    );
}

#[test]
fn trimming_happens_before_the_collapse_so_edge_blank_lines_vanish() {
    assert_eq!(normalize_like_server("\n\n\n\na\n\n\n\n"), "a");
}

#[test]
fn indentation_of_the_first_line_is_trimmed_too() {
    assert_eq!(normalize_like_server("   a\n   b"), "a\n   b");
}

#[test]
fn a_body_of_only_whitespace_becomes_empty() {
    assert_eq!(normalize_like_server(" \n\t\r\n "), "");
}

// --- the length the server checks ---------------------------------------------------------------

#[test]
fn the_limit_is_4000() {
    assert_eq!(MAX_BODY_LEN, 4000);
}

#[test]
fn an_empty_body_has_length_0() {
    assert_eq!(body_len(""), 0);
}

#[test]
fn a_body_of_only_whitespace_has_length_0() {
    assert_eq!(body_len(" \n\t "), 0);
}

#[test]
fn a_line_break_counts_as_one_character() {
    assert_eq!(body_len("a\r\nb"), 3);
}

#[test]
fn collapsed_blank_lines_count_as_two_newlines() {
    assert_eq!(body_len("a\n\n\n\n\nb"), 4);
}

#[test]
fn blank_lines_in_a_code_block_count_in_full() {
    assert_eq!(body_len("```\na\n\n\n\nb\n```"), 14);
}

#[test]
fn length_is_counted_in_code_points_not_bytes() {
    assert_eq!(body_len("日本語"), 3);
}

#[test]
fn a_family_emoji_counts_as_its_code_points() {
    assert_eq!(body_len("👨\u{200d}👩\u{200d}👧"), 5);
}

#[test]
fn exactly_4000_emoji_are_within_the_limit() {
    assert_eq!(body_len(&"🚀".repeat(4000)), 4000);
}

#[test]
fn exactly_4001_emoji_are_over_the_limit() {
    assert_eq!(body_len(&"🚀".repeat(4001)), 4001);
}

#[test]
fn padding_with_blank_lines_does_not_push_a_body_over() {
    let body = format!("{}\n\n\n\n\n\n\n{}", "a".repeat(1990), "b".repeat(2008));
    assert_eq!(body_len(&body), 4000);
}

#[test]
fn trimmed_edges_do_not_count() {
    assert_eq!(body_len(&format!("\n\n  {}  \n\n", "a".repeat(4000))), 4000);
}

#[test]
fn a_closing_fence_shorter_than_the_opening_one_still_closes() {
    assert_eq!(
        normalize_like_server("````\na\n\n\n\nb\n```\n\n\n\nc"),
        "````\na\n\n\n\nb\n```\n\nc"
    );
}

#[test]
fn a_blank_line_of_spaces_between_two_lines_is_kept() {
    assert_eq!(normalize_like_server("a\n \t\nb"), "a\n \t\nb");
}

#[test]
fn an_empty_code_block_on_two_lines_is_kept() {
    assert_eq!(normalize_like_server("```\n```"), "```\n```");
}

#[test]
fn blank_lines_before_an_indented_line_collapse_and_keep_the_indent() {
    assert_eq!(normalize_like_server("a\n\n\n  b"), "a\n\n  b");
}

#[test]
fn a_language_name_with_a_space_still_opens_a_block() {
    let body = "```rust ignore\na\n\n\n\nb\n```";
    assert_eq!(normalize_like_server(body), body);
}

#[test]
fn nul_and_the_mongolian_vowel_separator_are_trimmed() {
    assert_eq!(normalize_like_server("\0\u{180e}a\u{180e}\0"), "a");
}

#[test]
fn a_fence_right_after_the_opening_one_closes_only_when_no_later_line_does() {
    let body = "```\n```\na\n\n\n\nb\n```";
    assert_eq!(normalize_like_server(body), body);
}

#[test]
fn a_fence_right_after_the_opening_one_closes_the_block_when_it_is_the_only_one() {
    assert_eq!(normalize_like_server("```\n```\n\n\n\nb"), "```\n```\n\nb");
}
