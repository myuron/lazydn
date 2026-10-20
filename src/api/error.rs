use std::fmt;

/// What went wrong, so the UI can pick a message and decide whether to retry.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    /// The server answered with a non-success status.
    Http {
        /// HTTP status.
        status: u16,
        /// The API `code`, when the body had one.
        code: Option<String>,
        /// The API `message`, or the status text.
        message: String,
        /// Seconds from `Retry-After`, when sent.
        retry_after: Option<u64>,
        /// The `user` of a `user_blocked` body: the account that blocked the caller.
        blocked_user: Option<Box<crate::model::User>>,
    },
    /// The request did not complete: connection failure or timeout.
    Network(String),
    /// A success response could not be read: bad JSON, wrong shape or too large.
    Decode(String),
    /// The signer failed.
    Sign(String),
    /// Raised locally for a guest calling an endpoint that needs a signature.
    SignInRequired,
    /// A caller-supplied value would have changed which endpoint is addressed.
    InvalidArgument(String),
    /// The base URL given to the client cannot be used.
    InvalidBaseUrl,
}

/// The failure of one API call.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    /// The failure category.
    pub kind: ErrorKind,
}

impl ApiError {
    /// The API `code`, or `http_<status>`, `network`, `decode`, `sign`, `sign_in_required`,
    /// `invalid_argument`, `invalid_base_url`. Used for toasts and tests.
    pub fn code(&self) -> String {
        match &self.kind {
            ErrorKind::Http { status, code, .. } => {
                code.clone().unwrap_or_else(|| format!("http_{status}"))
            }
            ErrorKind::Network(_) => "network".into(),
            ErrorKind::Decode(_) => "decode".into(),
            ErrorKind::Sign(_) => "sign".into(),
            ErrorKind::SignInRequired => "sign_in_required".into(),
            ErrorKind::InvalidArgument(_) => "invalid_argument".into(),
            ErrorKind::InvalidBaseUrl => "invalid_base_url".into(),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}

impl std::error::Error for ApiError {}
