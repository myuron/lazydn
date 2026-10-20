use super::writes::write_failure_text;
use super::{App, Modal, ToastKind, toast};
use crate::action::{Effect, Failure};
use crate::dialogs::{DETAILS_MAX, REASONS, ReportFocus, ReportState};
use crate::editor::Editor;
use crate::feed::ReqId;
use crate::keymap::ReportKey;
use crate::text::is_unsafe_char;

fn clean_paste(text: &str, room: usize) -> String {
    text.replace("\r\n", " ")
        .chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .filter(|c| !is_unsafe_char(*c))
        .take(room)
        .collect()
}

/// Reporting a commit.
impl App {
    /// The open report dialog.
    pub fn report(&self) -> Option<&ReportState> {
        self.report.as_ref()
    }

    /// Whether the keys of the report dialog go to the details field.
    pub fn report_input_focused(&self) -> bool {
        self.modal == Some(Modal::Report)
            && self
                .report
                .as_ref()
                .is_some_and(|r| r.focus == ReportFocus::Details)
    }

    pub(super) fn open_report(&mut self) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        let Some(card) = self.selected_commit() else {
            return Vec::new();
        };
        // The answer of the report that was closed with Esc is still due.
        if !self.not_own(&card) || self.reports.contains_key(&card.id) {
            return Vec::new();
        }
        self.report = Some(ReportState {
            id: card.id,
            short_hash: card.short_hash,
            reason: REASONS[0],
            focus: ReportFocus::Reason,
            details: Editor::new(),
            sending: false,
            error: None,
        });
        self.modal = Some(Modal::Report);
        Vec::new()
    }

    pub(super) fn on_report_key(&mut self, key: ReportKey) -> Vec<Effect> {
        let Some(report) = self.report.as_mut().filter(|r| !r.sending) else {
            return Vec::new();
        };
        let in_details = report.focus == ReportFocus::Details;
        let editor = &mut report.details;
        match key {
            ReportKey::Submit => return self.send_report(),
            ReportKey::Tab => {
                report.focus = if in_details {
                    ReportFocus::Reason
                } else {
                    ReportFocus::Details
                };
            }
            ReportKey::Up | ReportKey::Down if !in_details => {
                let at = REASONS
                    .iter()
                    .position(|r| *r == report.reason)
                    .unwrap_or(0);
                let to = if key == ReportKey::Up {
                    at.saturating_sub(1)
                } else {
                    (at + 1).min(REASONS.len() - 1)
                };
                report.reason = REASONS[to];
            }
            ReportKey::Insert(c) if in_details => {
                if editor.char_count() < DETAILS_MAX {
                    editor.insert_char(c);
                }
            }
            ReportKey::Backspace if in_details => editor.backspace(),
            ReportKey::Delete if in_details => editor.delete(),
            ReportKey::Left if in_details => editor.left(),
            ReportKey::Right if in_details => editor.right(),
            ReportKey::Home if in_details => editor.home(),
            ReportKey::End if in_details => editor.end(),
            _ => {}
        }
        Vec::new()
    }

    pub(super) fn paste_into_report(&mut self, text: &str) {
        let Some(report) = self.report.as_mut().filter(|r| !r.sending) else {
            return;
        };
        let room = DETAILS_MAX.saturating_sub(report.details.char_count());
        report.details.paste(&clean_paste(text, room));
    }

    fn send_report(&mut self) -> Vec<Effect> {
        let Some(report) = self.report.as_mut().filter(|r| !r.sending) else {
            return Vec::new();
        };
        report.sending = true;
        report.error = None;
        let details = report.details.text();
        let details = Some(details.trim().to_string()).filter(|d| !d.is_empty());
        let (id, short_hash, reason) =
            (report.id.clone(), report.short_hash.clone(), report.reason);
        let req = self.new_req();
        self.reports.insert(id.clone(), (req, short_hash));
        vec![Effect::ReportCommit {
            id,
            reason,
            details,
            req,
        }]
    }

    /// Takes the request out of the table when `req` is the one that is out.
    fn end_report(&mut self, id: &str, req: ReqId) -> Option<String> {
        if self.reports.get(id).map(|(r, _)| *r) != Some(req) {
            return None;
        }
        self.reports.remove(id).map(|(_, hash)| hash)
    }

    fn report_is_open_for(&self, id: &str) -> bool {
        self.modal == Some(Modal::Report) && self.report.as_ref().is_some_and(|r| r.id == id)
    }

    pub(super) fn report_done(&mut self, id: &str, req: ReqId) {
        let Some(short_hash) = self.end_report(id, req) else {
            return;
        };
        if self.report_is_open_for(id) {
            self.close_modal();
        }
        let text = format!("Reported {}", toast::clean(&short_hash));
        self.show_toast(ToastKind::Success, text);
    }

    pub(super) fn report_failed(&mut self, id: &str, req: ReqId, failure: &Failure) {
        if self.end_report(id, req).is_none() {
            return;
        }
        if self.report_is_open_for(id) {
            let text = if failure.code == "network" {
                "network error — press Enter to retry".to_string()
            } else {
                write_failure_text(failure)
            };
            if let Some(report) = &mut self.report {
                report.sending = false;
                report.error = Some(text);
            }
            return;
        }
        self.show_toast(ToastKind::Error, write_failure_text(failure));
    }
}
