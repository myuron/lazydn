use super::{App, Screen, Session};
use crate::action::Effect;
use crate::feed::{LoadState, ReqId, settle_scroll};
use crate::model::ThreadItem;
use crate::thread::{Entry, ReplyList, item_id, item_replies};

/// Thread screen: requests, selection and scrolling.
impl App {
    pub(super) fn thread_id(&self) -> Option<String> {
        match &self.screen {
            Screen::Thread(id) => Some(id.clone()),
            _ => None,
        }
    }

    pub fn is_guest(&self) -> bool {
        matches!(self.session, Session::Guest)
    }

    pub(super) fn new_req(&mut self) -> ReqId {
        self.next_req += 1;
        ReqId(self.next_req)
    }

    /// Fetches the main commit and, for signed-in users, the first level of replies.
    pub(super) fn thread_requests(&mut self, id: &str) -> Vec<Effect> {
        let req = self.new_req();
        let state = self.threads.entry(id.to_string()).or_default();
        state.main_pending = Some(req);
        state.main_load = LoadState::Idle;
        let mut effects = vec![Effect::FetchThread {
            id: id.to_string(),
            req,
        }];
        if !self.is_guest() {
            effects.push(self.request_replies(id, id, None));
        }
        effects
    }

    pub(super) fn request_replies(
        &mut self,
        thread: &str,
        parent: &str,
        cursor: Option<String>,
    ) -> Effect {
        let req = self.new_req();
        if let Some(state) = self.threads.get_mut(thread) {
            state
                .children
                .entry(parent.to_string())
                .or_insert_with(ReplyList::default)
                .begin(req);
        }
        Effect::FetchReplies {
            thread: thread.to_string(),
            parent: parent.to_string(),
            cursor,
            req,
        }
    }

    /// A late response must not bring back a state that Goto or a refresh dropped.
    pub(super) fn reply_list_mut(&mut self, thread: &str, parent: &str) -> Option<&mut ReplyList> {
        self.threads.get_mut(thread)?.children.get_mut(parent)
    }

    /// Keeps the selection on the same row when a response adds or removes rows above it.
    pub(super) fn restore_selection(&mut self, thread: &str, kept: Option<Entry>) {
        // A show-more row keeps its index: it moves with the page that replaces it.
        let Some(kept) = kept.filter(|e| !matches!(e, Entry::ShowMore(_))) else {
            return;
        };
        let guest = self.is_guest();
        let Some(state) = self.threads.get_mut(thread) else {
            return;
        };
        if let Some(at) = state.entries(thread, guest).iter().position(|e| *e == kept) {
            state.selected = at;
        }
    }

    /// Back leaves a screen for good, so a thread that history can no longer reach would only go stale.
    pub(super) fn drop_unreachable_threads(&mut self) {
        let reachable: Vec<&String> = self
            .history
            .iter()
            .chain(std::iter::once(&self.screen))
            .filter_map(|s| match s {
                Screen::Thread(id) => Some(id),
                _ => None,
            })
            .collect();
        self.threads.retain(|id, _| reachable.contains(&id));
    }

    pub(super) fn selected_entry(&self, id: &str) -> Option<Entry> {
        let state = self.threads.get(id)?;
        state
            .entries(id, self.is_guest())
            .into_iter()
            .nth(state.selected)
    }

    pub(super) fn thread_select(&mut self, id: &str, index: usize) -> Vec<Effect> {
        let guest = self.is_guest();
        let Some(state) = self.threads.get_mut(id) else {
            return Vec::new();
        };
        let slots = state.slots(id, guest);
        let selectable: Vec<_> = slots.iter().filter(|s| s.entry.is_some()).collect();
        if selectable.is_empty() {
            return Vec::new();
        }
        state.selected = index.min(selectable.len() - 1);
        let top = selectable[state.selected].top;
        let cursor = top.and_then(|top| state.wants_prefetch(id, top));
        match cursor {
            Some(cursor) if !guest => vec![self.request_replies(id, id, Some(cursor))],
            _ => Vec::new(),
        }
    }

    pub(super) fn find_item(&self, id: &str, item: &str) -> Option<ThreadItem> {
        let state = self.threads.get(id)?;
        state
            .ancestors
            .iter()
            .chain(state.children.values().flat_map(|l| l.items.iter()))
            .find(|i| item_id(i) == item)
            .cloned()
    }

    pub(super) fn thread_open_selected(&mut self) -> Vec<Effect> {
        let Some(id) = self.thread_id() else {
            return Vec::new();
        };
        let target = match self.selected_entry(&id) {
            Some(Entry::Ancestor(i)) => self
                .threads
                .get(&id)
                .and_then(|t| t.ancestors.get(i).cloned()),
            Some(Entry::Continuation) => self
                .threads
                .get(&id)
                .and_then(|t| t.continuation(&id).cloned())
                .map(|c| ThreadItem::Commit(Box::new(c))),
            Some(Entry::Reply(item, _)) => self.find_item(&id, &item),
            Some(Entry::ShowMore(parent)) => return self.load_more(&id, &parent),
            Some(Entry::Main) | None => None,
        };
        match target {
            Some(ThreadItem::Commit(c)) => self.open(Screen::Thread(c.id)),
            _ => Vec::new(),
        }
    }

    pub(super) fn load_more(&mut self, thread: &str, parent: &str) -> Vec<Effect> {
        let cursor = self
            .threads
            .get(thread)
            .and_then(|t| t.children.get(parent))
            .filter(|l| l.pending.is_none())
            .and_then(|l| l.next_cursor.clone());
        match cursor {
            Some(cursor) => vec![self.request_replies(thread, parent, Some(cursor))],
            None => Vec::new(),
        }
    }

    pub(super) fn thread_toggle(&mut self) -> Vec<Effect> {
        let Some(id) = self.thread_id() else {
            return Vec::new();
        };
        let reply = match self.selected_entry(&id) {
            Some(Entry::Reply(reply, _)) => reply,
            Some(Entry::ShowMore(parent)) => return self.load_more(&id, &parent),
            _ => return Vec::new(),
        };
        let has_replies = self
            .find_item(&id, &reply)
            .is_some_and(|i| item_replies(&i) > 0);
        if !has_replies {
            return Vec::new();
        }
        let Some(state) = self.threads.get_mut(&id) else {
            return Vec::new();
        };
        let list = state.children.get(&reply);
        // A failed first load is retried by the key that opened it, instead of folding the empty list.
        let retry =
            list.is_some_and(|l| l.items.is_empty() && l.pending.is_none() && l.failed().is_some());
        let never_asked = list.is_none();
        if state.expanded.contains(&reply) && !retry {
            state.expanded.remove(&reply);
            return Vec::new();
        }
        state.expanded.insert(reply.clone());
        if never_asked || retry {
            return vec![self.request_replies(&id, &reply, None)];
        }
        Vec::new()
    }

    pub(super) fn reconcile_thread_scroll(&mut self, id: &str) {
        let guest = self.is_guest();
        let Some(state) = self.threads.get(id) else {
            return;
        };
        let count = state.entries(id, guest).len();
        let selected = state.selected.min(count.saturating_sub(1));
        let Some(main) = self.main_area() else {
            if let Some(state) = self.threads.get_mut(id) {
                state.selected = selected;
            }
            return;
        };
        let view = main.height.saturating_sub(2) as usize;
        let scroll = usize::from(state.scroll);
        let rows = crate::ui::thread::rows(self, id, main.width.saturating_sub(2));
        let wanted = match rows.entries.get(selected) {
            Some(span) if selected > 0 => {
                // The rows after an entry (hints, loading, errors) belong to it, so they scroll into view too.
                let height = match rows.entries.get(selected + 1) {
                    Some(next) => next.start - span.start,
                    None => rows.lines.len() - span.start,
                };
                settle_scroll((span.start, height), rows.lines.len(), scroll, view)
            }
            _ => 0,
        };
        if let Some(state) = self.threads.get_mut(id) {
            state.selected = selected;
            state.scroll = wanted.min(u16::MAX as usize) as u16;
        }
    }
}
