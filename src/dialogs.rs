//! The state of the two dialogs that guard a card: the confirmation of a delete and the report.

use crate::editor::Editor;
use crate::model::ReportReason;

/// Most code points the details of a report hold, counted as the server does.
pub const DETAILS_MAX: usize = 500;

/// The reasons in the order they are listed.
pub const REASONS: [ReportReason; 4] = [
    ReportReason::Spam,
    ReportReason::Harassment,
    ReportReason::Unsafe,
    ReportReason::Other,
];

/// A delete that waits for `y`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteConfirm {
    pub id: String,
    pub short_hash: String,
}

/// Where the keys of the report dialog go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFocus {
    Reason,
    Details,
}

/// An open report dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportState {
    pub id: String,
    pub short_hash: String,
    pub reason: ReportReason,
    pub focus: ReportFocus,
    pub details: Editor,
    /// Whether the report is out; the dialog takes no input then.
    pub sending: bool,
    /// Why the last attempt was refused.
    pub error: Option<String>,
}
