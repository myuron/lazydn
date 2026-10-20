use url::Url;

use crate::api::authorize::REDIRECT_URI;
use crate::text::is_unsafe_char;

/// Why the text in the Code field cannot be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeError {
    /// Nothing but whitespace; the dialog ignores Enter instead of complaining.
    Empty,
    NoCode,
    /// The code is not 64 lowercase hex characters.
    BadCode,
    /// A `devnull://` URL that is not the callback of this app.
    WrongUrl,
    StateMismatch,
    Denied,
}

impl CodeError {
    /// What the dialog says under the Code field.
    pub fn text(self) -> &'static str {
        match self {
            CodeError::Empty => "",
            CodeError::NoCode => "no code in the URL — paste the whole devnull:// URL",
            CodeError::BadCode => {
                "the code must be 64 hex characters — paste the whole devnull:// URL"
            }
            CodeError::WrongUrl => "unexpected URL — paste the devnull://lazydn/callback URL",
            CodeError::StateMismatch => "state mismatch — approve again",
            CodeError::Denied => "approval was cancelled",
        }
    }
}

/// Longest paste (and Code field) the dialog accepts, in characters.
pub const PASTE_MAX: usize = 2048;

const CODE_LEN: usize = 64;

fn is_code(text: &str) -> bool {
    text.len() == CODE_LEN && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Takes the approval code out of a pasted `devnull://` URL, or from the bare code.
/// Call it on Enter, with the `state` that went into the approval URL.
pub fn parse_code(input: &str, expected_state: &str) -> Result<String, CodeError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(CodeError::Empty);
    }
    if !input.starts_with("devnull://") {
        return if is_code(input) {
            Ok(input.to_string())
        } else {
            Err(CodeError::BadCode)
        };
    }
    let callback = Url::parse(input).map_err(|_| CodeError::WrongUrl)?;
    let expected = Url::parse(REDIRECT_URI).map_err(|_| CodeError::WrongUrl)?;
    let bare_authority = callback.port().is_none()
        && callback.username().is_empty()
        && callback.password().is_none();
    if callback.host_str() != expected.host_str()
        || callback.path() != expected.path()
        || !bare_authority
    {
        return Err(CodeError::WrongUrl);
    }
    let param = |name: &str| {
        callback
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if param("state").as_deref() != Some(expected_state) {
        return Err(CodeError::StateMismatch);
    }
    if param("error").as_deref() == Some("access_denied") {
        return Err(CodeError::Denied);
    }
    match param("code") {
        None => Err(CodeError::NoCode),
        Some(code) if is_code(&code) => Ok(code),
        Some(_) => Err(CodeError::BadCode),
    }
}

/// Drops control characters (newlines included) and bidi marks, and cuts the text to `PASTE_MAX`.
pub fn sanitize_paste(text: &str) -> String {
    text.chars()
        .filter(|c| !is_unsafe_char(*c))
        .take(PASTE_MAX)
        .collect()
}
