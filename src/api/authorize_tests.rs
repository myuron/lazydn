use super::authorize::{AuthorizeError, authorize_url};

const FINGERPRINT: &str = "SHA256:KhVovNmj/HaIYLFS2Thqiu7ov5FlDlXpnntLqnfTL0I";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

fn url_with(device_name: &str, redirect: &str) -> Result<url::Url, AuthorizeError> {
    authorize_url(
        "http://127.0.0.1:9",
        FINGERPRINT,
        device_name,
        CHALLENGE,
        "0123456789abcdef0123456789abcdef",
        redirect,
    )
}

#[test]
fn carries_every_parameter_on_the_mobile_authorize_path() {
    let url = url_with("lazydn on laptop", "devnull://callback").expect("builds");

    assert_eq!(url.path(), "/mobile/authorize");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    let mut expected = vec![
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("device_name", "lazydn on laptop"),
        ("fingerprint", FINGERPRINT),
        ("redirect_uri", "devnull://callback"),
        ("state", "0123456789abcdef0123456789abcdef"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(pairs, expected);
}

#[test]
fn values_are_percent_encoded_and_round_trip() {
    let name = "Ann's laptop & co=1";

    let url = url_with(name, "devnull://cb?x=1&y=2").expect("builds");

    let query = url.query().unwrap_or_default();
    assert!(!query.contains(' '), "{query}");
    assert!(!query.contains("://"), "{query}");
    assert_eq!(query.matches('&').count(), 5, "{query}");
    let got: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(got.get("device_name").map(String::as_str), Some(name));
    assert_eq!(
        got.get("redirect_uri").map(String::as_str),
        Some("devnull://cb?x=1&y=2")
    );
}

#[test]
fn rejects_a_redirect_that_does_not_start_with_the_devnull_scheme() {
    for bad in [
        "https://evil.example/cb",
        "devnull:/x",
        "",
        "xdevnull://cb",
        "devnull://cb#frag",
    ] {
        assert_eq!(
            url_with("d", bad),
            Err(AuthorizeError::InvalidRedirectUri),
            "{bad:?}"
        );
    }
}

#[test]
fn rejects_a_base_url_that_is_not_a_url() {
    let result = authorize_url(
        "not a url",
        FINGERPRINT,
        "d",
        CHALLENGE,
        "s",
        "devnull://cb",
    );

    assert_eq!(result, Err(AuthorizeError::InvalidBaseUrl));
}
