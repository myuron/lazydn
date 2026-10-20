use crate::signin::{CodeError, PASTE_MAX, parse_code, sanitize_paste};

const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const STATE: &str = "0f0e0d0c0b0a09080706050403020100";

fn callback(query: &str) -> String {
    format!("devnull://lazydn/callback?{query}")
}

// --- the code is read from a pasted URL ---------------------------------------------------------

#[test]
fn a_callback_url_with_a_matching_state_yields_its_code() {
    let url = callback(&format!("code={CODE}&state={STATE}"));
    assert_eq!(parse_code(&url, STATE), Ok(CODE.to_string()));
}

#[test]
fn the_order_of_the_query_parameters_does_not_matter() {
    let url = callback(&format!("state={STATE}&code={CODE}"));
    assert_eq!(parse_code(&url, STATE), Ok(CODE.to_string()));
}

#[test]
fn a_different_state_is_refused_as_a_mismatch() {
    let url = callback(&format!(
        "code={CODE}&state=ffffffffffffffffffffffffffffffff"
    ));
    assert_eq!(parse_code(&url, STATE), Err(CodeError::StateMismatch));
}

#[test]
fn a_callback_url_without_a_state_is_refused_as_a_mismatch() {
    let url = callback(&format!("code={CODE}"));
    assert_eq!(parse_code(&url, STATE), Err(CodeError::StateMismatch));
}

#[test]
fn access_denied_means_the_approval_was_cancelled() {
    let url = callback(&format!("error=access_denied&state={STATE}"));
    assert_eq!(parse_code(&url, STATE), Err(CodeError::Denied));
}

#[test]
fn a_cancellation_with_another_state_is_a_mismatch_first() {
    let url = callback("error=access_denied&state=ffffffffffffffffffffffffffffffff");
    assert_eq!(parse_code(&url, STATE), Err(CodeError::StateMismatch));
}

#[test]
fn a_callback_url_without_a_code_says_there_is_no_code() {
    let url = callback(&format!("state={STATE}"));
    assert_eq!(parse_code(&url, STATE), Err(CodeError::NoCode));
}

#[test]
fn a_url_for_another_host_or_path_is_not_our_callback() {
    for url in [
        format!("devnull://evil/callback?code={CODE}&state={STATE}"),
        format!("devnull://lazydn/other?code={CODE}&state={STATE}"),
    ] {
        assert_eq!(parse_code(&url, STATE), Err(CodeError::WrongUrl), "{url}");
    }
}

#[test]
fn the_code_in_a_url_must_be_64_lowercase_hex_characters() {
    for code in [
        "abc",
        &CODE[..63],
        &CODE.to_uppercase(),
        &format!("{CODE}0"),
    ] {
        let url = callback(&format!("code={code}&state={STATE}"));
        assert_eq!(parse_code(&url, STATE), Err(CodeError::BadCode), "{code}");
    }
}

// --- a bare code ----------------------------------------------------------------------------

#[test]
fn a_bare_code_is_taken_as_it_is() {
    assert_eq!(parse_code(CODE, STATE), Ok(CODE.to_string()));
}

#[test]
fn whitespace_around_the_input_is_dropped() {
    assert_eq!(
        parse_code(&format!("  {CODE} \n"), STATE),
        Ok(CODE.to_string())
    );
    let url = callback(&format!("code={CODE}&state={STATE}"));
    assert_eq!(
        parse_code(&format!("\t{url}  "), STATE),
        Ok(CODE.to_string())
    );
}

#[test]
fn a_bare_code_must_be_64_lowercase_hex_characters() {
    for input in [
        "hello",
        &CODE[..63],
        &CODE.to_uppercase(),
        "https://x.example/?code=1",
    ] {
        assert_eq!(parse_code(input, STATE), Err(CodeError::BadCode), "{input}");
    }
}

#[test]
fn an_empty_input_is_reported_as_empty() {
    for input in ["", "   ", "\n\t"] {
        assert_eq!(parse_code(input, STATE), Err(CodeError::Empty), "{input:?}");
    }
}

#[test]
fn the_messages_tell_the_user_what_to_do() {
    assert_eq!(
        CodeError::StateMismatch.text(),
        "state mismatch — approve again"
    );
    assert_eq!(CodeError::Denied.text(), "approval was cancelled");
    assert_eq!(
        CodeError::NoCode.text(),
        "no code in the URL — paste the whole devnull:// URL"
    );
    for error in [CodeError::BadCode, CodeError::WrongUrl] {
        assert!(!error.text().is_empty(), "{error:?}");
    }
    assert_ne!(CodeError::BadCode.text(), CodeError::WrongUrl.text());
}

// --- paste ----------------------------------------------------------------------------------

#[test]
fn a_paste_loses_its_newlines_and_other_control_characters() {
    assert_eq!(sanitize_paste("ab\ncd\r\n\tef\x1b\x7fgh"), "abcdefgh");
}

#[test]
fn a_paste_keeps_ordinary_text_untouched() {
    let url = callback(&format!("code={CODE}&state={STATE}"));
    assert_eq!(sanitize_paste(&url), url);
}

#[test]
fn a_paste_is_cut_at_2kb_after_the_control_characters_are_gone() {
    assert_eq!(PASTE_MAX, 2048);
    assert_eq!(sanitize_paste(&"a".repeat(5000)).chars().count(), 2048);
    assert_eq!(sanitize_paste(&format!("{}x", "\n".repeat(3000))), "x");
}

#[test]
fn a_callback_url_with_a_port_or_userinfo_is_not_our_callback() {
    for url in [
        format!("devnull://lazydn:8080/callback?code={CODE}&state={STATE}"),
        format!("devnull://user@lazydn/callback?code={CODE}&state={STATE}"),
        format!("devnull://user:pw@lazydn/callback?code={CODE}&state={STATE}"),
    ] {
        assert_eq!(parse_code(&url, STATE), Err(CodeError::WrongUrl), "{url}");
    }
}

#[test]
fn a_paste_loses_bidi_marks_too() {
    assert_eq!(
        sanitize_paste("a\u{202e}b\u{2066}c\u{200f}d\u{200e}e\u{202a}f\u{2069}g"),
        "abcdefg"
    );
}

#[test]
fn a_paste_loses_the_arabic_letter_mark_and_line_separators() {
    assert_eq!(sanitize_paste("a\u{061c}b"), "ab");
    assert_eq!(sanitize_paste("a\u{2028}b"), "ab");
    assert_eq!(sanitize_paste("a\u{2029}b"), "ab");
}
