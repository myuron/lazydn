use url::Url;

/// Why an authorize URL could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizeError {
    /// The web base URL does not parse or cannot carry a path.
    InvalidBaseUrl,
    /// The redirect is not a `devnull://` URL without a fragment.
    InvalidRedirectUri,
}

impl std::fmt::Display for AuthorizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthorizeError::InvalidBaseUrl => write!(f, "invalid base URL"),
            AuthorizeError::InvalidRedirectUri => write!(f, "invalid redirect URI"),
        }
    }
}

impl std::error::Error for AuthorizeError {}

const REDIRECT_PREFIX: &str = "devnull://";

/// The page the user opens in a browser to approve this device; call it once per sign-in attempt.
pub fn authorize_url(
    base_web_url: &str,
    fingerprint: &str,
    device_name: &str,
    challenge: &str,
    state: &str,
    redirect_uri: &str,
) -> Result<Url, AuthorizeError> {
    if !redirect_uri.starts_with(REDIRECT_PREFIX) || redirect_uri.contains('#') {
        return Err(AuthorizeError::InvalidRedirectUri);
    }
    let mut url = Url::parse(base_web_url).map_err(|_| AuthorizeError::InvalidBaseUrl)?;
    if url.cannot_be_a_base() {
        return Err(AuthorizeError::InvalidBaseUrl);
    }
    url.set_path("/mobile/authorize");
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("fingerprint", fingerprint)
        .append_pair("device_name", device_name)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("redirect_uri", redirect_uri);
    Ok(url)
}

/// Where the approval page sends the user back to; the dialog expects a pasted URL of this shape.
pub const REDIRECT_URI: &str = "devnull://lazydn/callback";
