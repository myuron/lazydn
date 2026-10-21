use super::App;
use crate::api::client::RateLimit;

/// What the status line says about the connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConnState {
    /// No response has arrived yet.
    #[default]
    Connecting,
    Online,
    Offline,
}

/// What the status line shows about the API: reachability, budget and target host.
impl App {
    /// Whether the last request reached the server; `Connecting` until the first one finishes.
    pub fn conn_state(&self) -> ConnState {
        self.conn
    }

    /// The last rate limit the server reported.
    pub fn rate(&self) -> Option<RateLimit> {
        self.rate
    }

    /// Set only when the base URL is not the default, so that the user sees where requests go.
    pub fn host_label(&self) -> Option<&str> {
        self.host_label.as_deref()
    }

    /// `None` means the default host, which the status line does not name.
    pub fn set_host_label(&mut self, host: Option<String>) {
        self.host_label = host;
    }

    /// An answer without a rate (an error response, say) keeps the last known one.
    pub(super) fn apply_api_status(&mut self, rate: Option<RateLimit>, online: bool) {
        self.conn = if online {
            ConnState::Online
        } else {
            ConnState::Offline
        };
        if rate.is_some() {
            self.rate = rate;
        }
    }
}
