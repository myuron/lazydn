use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER, USER_AGENT};
use reqwest::{Method, redirect};
use serde::de::DeserializeOwned;
use url::Url;

use crate::model::{
    BlockResponse, Commit, GroupPage, LaneSetting, LogPage, MeResponse, MuteResponse,
    NotificationsPage, Page, PastelResponse, ReportReason, Rgb, ThreadItem, ThreadResponse, User,
    UserResponse,
};

use super::clock::Clock;
use super::error::{ApiError, ErrorKind};
use super::request::sign_request;
use super::sign::Signer;

/// The only place the production host is written down.
pub const PRODUCTION_BASE_URL: &str = "https://dev-null.sh";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// Far above any real page, so a hostile or broken server cannot exhaust memory.
const DEFAULT_BODY_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The server's per-window request budget, as read from the response headers; show it in the status line.
pub struct RateLimit {
    /// Requests allowed in the window.
    pub limit: u32,
    /// Requests left in the window.
    pub remaining: u32,
}

#[derive(Debug, Clone, PartialEq)]
/// A decoded body with the rate-limit headers that came with it; the headers are absent on some responses.
pub struct Response<T> {
    /// The decoded body.
    pub data: T,
    /// Present only when the server sent both rate-limit headers.
    pub rate: Option<RateLimit>,
}

/// Talks to the dev-null API; one per signed-in or guest session. Every method is a single request with no retry.
pub struct ApiClient {
    base: Url,
    signer: Option<Arc<dyn Signer>>,
    clock: Arc<dyn Clock>,
    http: reqwest::Client,
    timeout: Duration,
    body_limit: usize,
}

/// The `{"data": ...}` envelope some endpoints put around their result.
#[derive(serde::Deserialize)]
struct Wrapped<T> {
    data: T,
}

/// Serializes a request body once, so the signed bytes and the sent bytes are the same.
fn json_body(value: &serde_json::Value) -> Vec<u8> {
    // A Value with string keys always serializes, so there is no error to surface.
    serde_json::to_vec(value).expect("a JSON value serializes")
}

/// A caller-supplied value the client refuses before any request is made.
fn invalid(msg: &str) -> ApiError {
    ApiError {
        kind: ErrorKind::InvalidArgument(msg.to_string()),
    }
}

type ApiResult<T> = Result<Response<T>, ApiError>;

/// Whether the server rejects the call without a signature.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Auth {
    Required,
    Optional,
}

fn network(msg: impl ToString) -> ApiError {
    ApiError {
        kind: ErrorKind::Network(msg.to_string()),
    }
}

fn build_http() -> Result<reqwest::Client, ApiError> {
    reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .map_err(network)
}

fn header_u64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

fn rate_limit(headers: &reqwest::header::HeaderMap) -> Option<RateLimit> {
    Some(RateLimit {
        limit: u32::try_from(header_u64(headers, "x-ratelimit-limit")?).ok()?,
        remaining: u32::try_from(header_u64(headers, "x-ratelimit-remaining")?).ok()?,
    })
}

fn cursor_query(cursor: Option<&str>) -> Vec<(&'static str, String)> {
    cursor
        .map(|c| ("cursor", c.to_string()))
        .into_iter()
        .collect()
}

impl ApiClient {
    /// `base_url` is the host root only (`http://host:port`, optionally with a trailing `/`),
    /// with no path prefix; the caller supplies it so tests can point at a mock.
    pub fn new(
        base_url: &str,
        signer: Option<Arc<dyn Signer>>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ApiError> {
        let bad_base = || ApiError {
            kind: ErrorKind::InvalidBaseUrl,
        };
        let base = Url::parse(base_url).map_err(|_| bad_base())?;
        if base.cannot_be_a_base() {
            return Err(bad_base());
        }
        Ok(ApiClient {
            base,
            signer,
            clock,
            http: build_http()?,
            timeout: DEFAULT_TIMEOUT,
            body_limit: DEFAULT_BODY_LIMIT,
        })
    }

    /// Overrides the 15 second request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Overrides the response size cap, mainly so tests can hit it cheaply.
    pub fn with_body_limit(mut self, limit: usize) -> Self {
        self.body_limit = limit;
        self
    }

    /// Refuses segments the url crate would silently rewrite (empty, `.`, `..`) or that
    /// smuggle a separator, since they could address, and sign for, another endpoint.
    fn url(&self, segments: &[&str], query: &[(&str, String)]) -> Result<Url, ApiError> {
        if let Some(bad) = segments
            .iter()
            .find(|s| s.is_empty() || **s == "." || **s == ".." || s.contains(['/', '\\']))
        {
            return Err(invalid(&format!("bad path segment {bad:?}")));
        }
        let mut url = self.base.clone();
        url.set_query(None);
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().push("api").push("v1").extend(segments);
        }
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<Vec<u8>>,
        auth: Auth,
    ) -> ApiResult<T> {
        let (bytes, rate) = self.send_raw(method, segments, query, body, auth).await?;
        let data = serde_json::from_slice(&bytes).map_err(|e| ApiError {
            kind: ErrorKind::Decode(e.to_string()),
        })?;
        Ok(Response { data, rate })
    }

    /// For endpoints documented to answer 204. A 204 body is not read; any other 2xx body is read within the size limit
    /// and discarded without decoding, so extra content never turns success into an error.
    async fn send_unit(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Vec<u8>>,
    ) -> ApiResult<()> {
        let (_, rate) = self
            .send_raw(method, segments, &[], body, Auth::Required)
            .await?;
        Ok(Response { data: (), rate })
    }

    /// Signed endpoints that wrap their result as `{"data": ...}`.
    async fn send_wrapped<T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Vec<u8>>,
    ) -> ApiResult<T> {
        let r: Response<Wrapped<T>> = self
            .send(method, segments, &[], body, Auth::Required)
            .await?;
        Ok(Response {
            data: r.data.data,
            rate: r.rate,
        })
    }

    /// Sends a signed request whose response is not wrapped in `data`.
    async fn send_signed<T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Vec<u8>>,
    ) -> ApiResult<T> {
        self.send(method, segments, &[], body, Auth::Required).await
    }

    /// Does the transport work shared by every call: signing, sending, the size limit and status handling.
    /// Returns the raw body, empty for 204.
    async fn send_raw(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<Vec<u8>>,
        auth: Auth,
    ) -> Result<(Vec<u8>, Option<RateLimit>), ApiError> {
        if auth == Auth::Required && self.signer.is_none() {
            return Err(ApiError {
                kind: ErrorKind::SignInRequired,
            });
        }
        let url = self.url(segments, query)?;
        let mut req = self
            .http
            .request(method.clone(), url.clone())
            .timeout(self.timeout)
            .header(USER_AGENT, concat!("lazydn/", env!("CARGO_PKG_VERSION")))
            .header(ACCEPT, "application/json");
        let body_bytes = body.unwrap_or_default();
        if let Some(signer) = &self.signer {
            let mut target = url.path().to_string();
            if let Some(q) = url.query() {
                target.push('?');
                target.push_str(q);
            }
            let h = sign_request(
                signer.as_ref(),
                self.clock.as_ref(),
                method.as_str(),
                &target,
                &body_bytes,
            )?;
            req = req
                .header("X-Devnull-Timestamp", h.timestamp)
                .header("X-Devnull-Nonce", h.nonce)
                .header("X-SSH-Signature", h.signature);
        }
        if !body_bytes.is_empty() {
            req = req
                .header(CONTENT_TYPE, "application/json")
                .body(body_bytes);
        }
        let resp = req.send().await.map_err(network)?;
        let status = resp.status();
        let headers = resp.headers().clone();
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok((Vec::new(), rate_limit(&headers)));
        }
        let bytes = self.read_limited(resp).await?;
        if !status.is_success() {
            return Err(http_error(status, &headers, &bytes));
        }
        Ok((bytes, rate_limit(&headers)))
    }

    /// Counts while reading, since a missing or false Content-Length proves nothing.
    async fn read_limited(&self, mut resp: reqwest::Response) -> Result<Vec<u8>, ApiError> {
        let too_large = || ApiError {
            kind: ErrorKind::Decode(format!(
                "response exceeds the {} byte limit",
                self.body_limit
            )),
        };
        if resp
            .content_length()
            .is_some_and(|n| n > self.body_limit as u64)
        {
            return Err(too_large());
        }
        let mut out = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(network)? {
            if out.len() + chunk.len() > self.body_limit {
                return Err(too_large());
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    async fn get<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        auth: Auth,
    ) -> ApiResult<T> {
        self.send(Method::GET, segments, query, None, auth).await
    }

    /// Current user (signature required; a guest gets `sign_in_required`). Call after sign-in and to refresh the unread count.
    pub async fn me(&self) -> ApiResult<MeResponse> {
        self.get(&["me"], &[], Auth::Required).await
    }

    /// One page of a log such as `latest` or `following`; `since` counts newer commits (signature required; a guest gets `sign_in_required`).
    pub async fn log(
        &self,
        log: &str,
        since: Option<&str>,
        cursor: Option<&str>,
    ) -> ApiResult<LogPage> {
        let mut query: Vec<(&str, String)> = since
            .map(|s| ("since", s.to_string()))
            .into_iter()
            .collect();
        query.extend(cursor_query(cursor));
        self.get(&["log", log], &query, Auth::Required).await
    }

    /// A commit with its ancestors (signature optional; signing adds the ancestors and viewer fields).
    pub async fn commit(&self, id: &str) -> ApiResult<ThreadResponse> {
        self.get(&["commits", id], &[], Auth::Optional).await
    }

    /// One page of replies to a commit (signature required; a guest gets `sign_in_required`).
    pub async fn replies(&self, id: &str, cursor: Option<&str>) -> ApiResult<Page<ThreadItem>> {
        self.get(
            &["commits", id, "replies"],
            &cursor_query(cursor),
            Auth::Required,
        )
        .await
    }

    /// A profile (signature optional; signing adds the viewer's relationship fields).
    pub async fn user(&self, username: &str) -> ApiResult<UserResponse> {
        self.get(&["users", username], &[], Auth::Optional).await
    }

    /// One page of a user's commits, or their replies when `replies` is set (signature optional).
    pub async fn user_commits(
        &self,
        username: &str,
        replies: bool,
        cursor: Option<&str>,
    ) -> ApiResult<Page<Commit>> {
        let mut query = Vec::new();
        if replies {
            query.push(("replies", "true".to_string()));
        }
        query.extend(cursor_query(cursor));
        self.get(&["users", username, "commits"], &query, Auth::Optional)
            .await
    }

    /// One page of a user's followers (signature optional).
    pub async fn followers(&self, username: &str, cursor: Option<&str>) -> ApiResult<Page<User>> {
        self.get(
            &["users", username, "followers"],
            &cursor_query(cursor),
            Auth::Optional,
        )
        .await
    }

    /// One page of the users a user follows (signature optional).
    pub async fn following(&self, username: &str, cursor: Option<&str>) -> ApiResult<Page<User>> {
        self.get(
            &["users", username, "following"],
            &cursor_query(cursor),
            Auth::Optional,
        )
        .await
    }

    /// One page of notifications; as a side effect the server marks them all read (signature required; a guest gets `sign_in_required`).
    pub async fn notifications(
        &self,
        types: &str,
        cursor: Option<&str>,
    ) -> ApiResult<NotificationsPage> {
        let mut query = vec![("type", types.to_string())];
        query.extend(cursor_query(cursor));
        self.get(&["notifications"], &query, Auth::Required).await
    }

    /// One page of the notifications grouped with `id`; does not mark anything read (signature required; a guest gets `sign_in_required`).
    pub async fn notification_group(
        &self,
        id: &str,
        types: &str,
        cursor: Option<&str>,
    ) -> ApiResult<GroupPage> {
        let mut query = vec![("type", types.to_string())];
        query.extend(cursor_query(cursor));
        self.get(&["notifications", id, "group"], &query, Auth::Required)
            .await
    }

    /// One page of bookmarked commits (signature required; a guest gets `sign_in_required`).
    pub async fn saved(&self, cursor: Option<&str>) -> ApiResult<Page<Commit>> {
        self.get(&["saved"], &cursor_query(cursor), Auth::Required)
            .await
    }

    /// One page of muted users (signature required; a guest gets `sign_in_required`).
    pub async fn muted(&self, cursor: Option<&str>) -> ApiResult<Page<User>> {
        self.get(&["me", "muted"], &cursor_query(cursor), Auth::Required)
            .await
    }

    /// One page of blocked users (signature required; a guest gets `sign_in_required`).
    pub async fn blocked(&self, cursor: Option<&str>) -> ApiResult<Page<User>> {
        self.get(&["me", "blocked"], &cursor_query(cursor), Auth::Required)
            .await
    }

    /// Exchanges the authorize `code` and PKCE `verifier` for a device registration, signed with the approved key (signature required; a guest gets `sign_in_required`).
    pub async fn register_device(&self, code: &str, verifier: &str) -> ApiResult<MeResponse> {
        let body = serde_json::json!({"code": code, "code_verifier": verifier});
        let bytes = serde_json::to_vec(&body).map_err(|e| ApiError {
            kind: ErrorKind::Decode(e.to_string()),
        })?;
        self.send_signed(Method::POST, &["devices"], Some(bytes))
            .await
    }
}

impl ApiClient {
    /// Posts a commit, as a reply when `parent_id` is set or a quote when `quoted_commit_id` is set, never both
    /// (signature required; a guest gets `sign_in_required`).
    ///
    /// An empty `body`, an empty id, or both ids is `invalid_argument` without a request, even for a
    /// guest; length limits are left to the server.
    pub async fn create_commit(
        &self,
        body: &str,
        parent_id: Option<&str>,
        quoted_commit_id: Option<&str>,
    ) -> ApiResult<Commit> {
        if body.is_empty() {
            return Err(invalid("body is empty"));
        }
        if parent_id.is_some() && quoted_commit_id.is_some() {
            return Err(invalid("a commit cannot both reply and quote"));
        }
        // The server turns an empty id into null, which would post a reply as a top-level commit.
        if parent_id == Some("") || quoted_commit_id == Some("") {
            return Err(invalid("empty commit id"));
        }
        let mut json = serde_json::json!({"body": body});
        if let Some(p) = parent_id {
            json["parent_id"] = p.into();
        }
        if let Some(q) = quoted_commit_id {
            json["quoted_commit_id"] = q.into();
        }
        self.send_wrapped(Method::POST, &["commits"], Some(json_body(&json)))
            .await
    }

    /// Deletes the viewer's own commit (signature required; a guest gets `sign_in_required`).
    pub async fn delete_commit(&self, id: &str) -> ApiResult<()> {
        self.send_unit(Method::DELETE, &["commits", id], None).await
    }

    /// Stars a commit; returns it with the new counts (signature required; a guest gets `sign_in_required`).
    pub async fn star(&self, id: &str) -> ApiResult<Commit> {
        self.send_wrapped(Method::POST, &["commits", id, "star"], None)
            .await
    }

    /// Removes the viewer's star (signature required; a guest gets `sign_in_required`).
    pub async fn unstar(&self, id: &str) -> ApiResult<Commit> {
        self.send_wrapped(Method::DELETE, &["commits", id, "star"], None)
            .await
    }

    /// Saves a commit privately (signature required; a guest gets `sign_in_required`).
    pub async fn bookmark(&self, id: &str) -> ApiResult<Commit> {
        self.send_wrapped(Method::PUT, &["commits", id, "bookmark"], None)
            .await
    }

    /// Removes a commit from the saved list (signature required; a guest gets `sign_in_required`).
    pub async fn unbookmark(&self, id: &str) -> ApiResult<Commit> {
        self.send_wrapped(Method::DELETE, &["commits", id, "bookmark"], None)
            .await
    }

    /// Gives one pastel from the daily allowance (signature required; a guest gets `sign_in_required`).
    pub async fn give_pastel(&self, id: &str) -> ApiResult<PastelResponse> {
        self.send_signed(Method::POST, &["commits", id, "pasteis"], None)
            .await
    }

    /// Reports a commit; `details` is sent as given, up to 500 characters by the server's rule (signature required; a guest gets `sign_in_required`).
    pub async fn report(
        &self,
        id: &str,
        reason: ReportReason,
        details: Option<&str>,
    ) -> ApiResult<()> {
        let mut body = serde_json::json!({"reason": reason.as_str()});
        if let Some(d) = details {
            body["details"] = d.into();
        }
        self.send_unit(
            Method::POST,
            &["commits", id, "reports"],
            Some(json_body(&body)),
        )
        .await
    }

    /// Follows a user (signature required; a guest gets `sign_in_required`).
    pub async fn follow(&self, username: &str) -> ApiResult<()> {
        self.send_unit(Method::POST, &["users", username, "follow"], None)
            .await
    }

    /// Unfollows a user (signature required; a guest gets `sign_in_required`).
    pub async fn unfollow(&self, username: &str) -> ApiResult<()> {
        self.send_unit(Method::DELETE, &["users", username, "follow"], None)
            .await
    }

    /// Mutes a user (signature required; a guest gets `sign_in_required`).
    pub async fn mute(&self, username: &str) -> ApiResult<MuteResponse> {
        self.send_signed(Method::POST, &["users", username, "mute"], None)
            .await
    }

    /// Unmutes a user (signature required; a guest gets `sign_in_required`).
    pub async fn unmute(&self, username: &str) -> ApiResult<MuteResponse> {
        self.send_signed(Method::DELETE, &["users", username, "mute"], None)
            .await
    }

    /// Blocks a user (signature required; a guest gets `sign_in_required`).
    pub async fn block(&self, username: &str) -> ApiResult<BlockResponse> {
        self.send_signed(Method::POST, &["users", username, "block"], None)
            .await
    }

    /// Unblocks a user (signature required; a guest gets `sign_in_required`).
    pub async fn unblock(&self, username: &str) -> ApiResult<BlockResponse> {
        self.send_signed(Method::DELETE, &["users", username, "block"], None)
            .await
    }

    /// Sets the viewer's lane; a palette index above 5 is `invalid_argument` without a request, even for a guest
    /// (signature required; a guest gets `sign_in_required`).
    pub async fn update_lane(&self, lane: LaneSetting) -> ApiResult<MeResponse> {
        let body = match lane {
            LaneSetting::Automatic => serde_json::json!({"lane": null}),
            LaneSetting::Palette(n) if n <= 5 => serde_json::json!({"lane": n}),
            LaneSetting::Palette(n) => return Err(invalid(&format!("lane {n} is not in 0..=5"))),
            LaneSetting::Color(Rgb(r, g, b)) => {
                serde_json::json!({"lane": null, "color": format!("#{r:02x}{g:02x}{b:02x}")})
            }
        };
        self.send_signed(Method::PUT, &["me", "lane"], Some(json_body(&body)))
            .await
    }

    /// Turns pastéis mode on or off (signature required; a guest gets `sign_in_required`).
    pub async fn update_pasteis_mode(&self, enabled: bool) -> ApiResult<MeResponse> {
        let body = json_body(&serde_json::json!({"enabled": enabled}));
        self.send_signed(Method::PUT, &["me", "pasteis-mode"], Some(body))
            .await
    }

    /// Re-syncs the profile from GitHub, once a minute at most (signature required; a guest gets `sign_in_required`).
    pub async fn sync_github(&self) -> ApiResult<MeResponse> {
        self.send_signed(Method::PUT, &["me", "github"], None).await
    }

    /// Deletes the account and every device; irreversible (signature required; a guest gets `sign_in_required`).
    pub async fn delete_account(&self) -> ApiResult<()> {
        self.send_unit(Method::DELETE, &["me"], None).await
    }
}

fn http_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> ApiError {
    let json: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    let field = |name: &str| {
        json.as_ref()
            .and_then(|v| v.get(name))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    ApiError {
        kind: ErrorKind::Http {
            status: status.as_u16(),
            code: field("code"),
            message: field("message")
                .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_string()),
            retry_after: header_u64(headers, RETRY_AFTER.as_str()),
        },
    }
}
