use std::collections::{HashMap, HashSet};

use crate::feed::{LoadState, ReqId, near_end};
use crate::model::{Commit, ThreadItem};

/// One page-able list of replies under a single parent.
#[derive(Debug, Clone, Default)]
pub struct ReplyList {
    pub items: Vec<ThreadItem>,
    pub next_cursor: Option<String>,
    pub pending: Option<ReqId>,
    pub load: LoadState,
}

impl ReplyList {
    pub fn begin(&mut self, req: ReqId) {
        self.pending = Some(req);
        self.load = LoadState::Idle;
    }

    pub fn failed(&self) -> Option<&str> {
        match &self.load {
            LoadState::Failed { code, .. } => Some(code),
            LoadState::Idle => None,
        }
    }

    fn append_failed(&self) -> bool {
        matches!(self.load, LoadState::Failed { append: true, .. })
    }

    pub fn apply_loaded(&mut self, req: ReqId, items: Vec<ThreadItem>, cursor: Option<String>) {
        if self.pending != Some(req) {
            return;
        }
        for item in items {
            let id = item_id(&item);
            if !self.items.iter().any(|i| item_id(i) == id) {
                self.items.push(item);
            }
        }
        self.next_cursor = cursor;
        self.pending = None;
        self.load = LoadState::Idle;
    }

    pub fn apply_failed(&mut self, req: ReqId, code: &str) -> bool {
        if self.pending != Some(req) {
            return false;
        }
        self.pending = None;
        self.load = LoadState::Failed {
            code: code.to_string(),
            append: !self.items.is_empty(),
        };
        true
    }
}

pub fn item_id(item: &ThreadItem) -> &str {
    match item {
        ThreadItem::Commit(c) => &c.id,
        ThreadItem::Tombstone(t) => &t.id,
    }
}

pub(crate) fn item_replies(item: &ThreadItem) -> u32 {
    match item {
        ThreadItem::Commit(c) => c.replies_count,
        ThreadItem::Tombstone(t) => t.replies_count,
    }
    .unwrap_or(0)
}

/// The last 7 characters of the id without hyphens, which is what the web UI shows as the short hash.
pub fn short_hash(id: &str) -> String {
    let plain: Vec<char> = id.chars().filter(|c| *c != '-').collect();
    plain[plain.len().saturating_sub(7)..].iter().collect()
}

#[derive(Debug, Clone, Default)]
pub struct ThreadState {
    pub main: Option<ThreadItem>,
    pub ancestors: Vec<ThreadItem>,
    pub main_pending: Option<ReqId>,
    pub main_load: LoadState,
    /// Keyed by parent id; the thread's own id holds the first level.
    pub children: HashMap<String, ReplyList>,
    pub expanded: HashSet<String>,
    pub selected: usize,
    pub scroll: u16,
}

/// What a selectable row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Ancestor(usize),
    Main,
    Continuation,
    /// Reply id and the id of the list it belongs to.
    Reply(String, String),
    /// The row that loads the next page of the list under this parent.
    ShowMore(String),
}

#[derive(Debug, Clone)]
pub enum SlotKind {
    Rail,
    Ancestor(ThreadItem),
    Main {
        pos: Option<(u16, u16)>,
    },
    Continuation(Box<Commit>),
    Reply {
        item: ThreadItem,
        /// "Is last sibling" per displayed level, outermost first, including the item's own level.
        lasts: Vec<bool>,
        replying_to: Option<String>,
        quote: bool,
        has_children: bool,
    },
    Hint {
        lasts: Vec<bool>,
        count: u32,
    },
    /// `text` is the whole content after the dotted marker.
    Note {
        lasts: Vec<bool>,
        text: String,
    },
    Error {
        lasts: Vec<bool>,
        code: String,
        retry_key: char,
    },
    SignIn,
}

#[derive(Debug, Clone)]
pub struct Slot {
    pub kind: SlotKind,
    pub entry: Option<Entry>,
    /// Index in the first level that owns this row, used to decide when to prefetch.
    pub top: Option<usize>,
}

impl Slot {
    fn plain(kind: SlotKind) -> Self {
        Self {
            kind,
            entry: None,
            top: None,
        }
    }
}

fn is_continuation(item: &ThreadItem, main: &Commit) -> bool {
    match item {
        ThreadItem::Commit(c) => {
            c.author.username == main.author.username
                && c.parent_id.as_deref() == Some(main.id.as_str())
                && c.quoted_commit_id.as_deref() != Some(main.id.as_str())
        }
        ThreadItem::Tombstone(_) => false,
    }
}

/// What the lists of replies need to know of the main item, which may be a tombstone.
#[derive(Clone, Copy)]
struct MainInfo<'a> {
    id: &'a str,
    replies: u32,
}

/// Where a list sits in the tree: the owner's guides, the level of its items and the flat column's last id.
#[derive(Clone, Copy)]
struct Level<'a> {
    owner_lasts: &'a [bool],
    depth: usize,
    last3: Option<&'a str>,
    owner_top: Option<usize>,
}

impl ThreadState {
    /// The author's own first direct reply, which is drawn right under the main commit.
    pub fn continuation(&self, thread: &str) -> Option<&Commit> {
        let main = self.main_commit()?;
        self.children
            .get(thread)?
            .items
            .iter()
            .find(|i| is_continuation(i, main))
            .and_then(|i| match i {
                ThreadItem::Commit(c) => Some(c.as_ref()),
                ThreadItem::Tombstone(_) => None,
            })
    }

    fn first_level(&self, thread: &str) -> Vec<&ThreadItem> {
        let cont = self.continuation(thread).map(|c| c.id.clone());
        self.children
            .get(thread)
            .map(|l| {
                l.items
                    .iter()
                    .filter(|i| Some(item_id(i)) != cont.as_deref())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn first_level_len(&self, thread: &str) -> usize {
        self.first_level(thread).len()
    }

    /// Returns the cursor of the next first-level page when a selection owned by `top` is close enough to the end.
    pub fn wants_prefetch(&self, thread: &str, top: usize) -> Option<String> {
        let list = self.children.get(thread)?;
        let len = self.first_level_len(thread);
        (near_end(top, len) && list.pending.is_none() && !list.append_failed())
            .then(|| list.next_cursor.clone())
            .flatten()
    }

    /// The main commit while it exists; a deleted one has none.
    pub fn main_commit(&self) -> Option<&Commit> {
        match self.main.as_ref()? {
            ThreadItem::Commit(c) => Some(c),
            ThreadItem::Tombstone(_) => None,
        }
    }

    pub fn main_failed(&self) -> bool {
        matches!(self.main_load, LoadState::Failed { .. })
    }

    pub fn entries(&self, thread: &str, guest: bool) -> Vec<Entry> {
        self.slots(thread, guest)
            .into_iter()
            .filter_map(|s| s.entry)
            .collect()
    }

    pub fn slots(&self, thread: &str, guest: bool) -> Vec<Slot> {
        // A failed refresh hides the stale commit, so nothing below it may stay selectable.
        let Some(main_item) = self.main.as_ref().filter(|_| !self.main_failed()) else {
            return Vec::new();
        };
        let main = MainInfo {
            id: item_id(main_item),
            replies: item_replies(main_item),
        };
        let mut out = Vec::new();
        for (i, item) in self.ancestors.iter().enumerate() {
            out.push(Slot::plain(SlotKind::Rail));
            out.push(Slot {
                kind: SlotKind::Ancestor(item.clone()),
                entry: Some(Entry::Ancestor(i)),
                top: None,
            });
        }
        let cont = self.continuation(thread);
        out.push(Slot::plain(SlotKind::Rail));
        out.push(Slot {
            kind: SlotKind::Main {
                pos: cont.map(|_| (1, 2)),
            },
            entry: Some(Entry::Main),
            top: None,
        });
        if guest {
            if main.replies > 0 {
                out.push(Slot::plain(SlotKind::SignIn));
            }
            return out;
        }
        if let Some(c) = cont {
            out.push(Slot::plain(SlotKind::Rail));
            out.push(Slot {
                kind: SlotKind::Continuation(Box::new(c.clone())),
                entry: Some(Entry::Continuation),
                top: None,
            });
        }
        let mut replies = Vec::new();
        let root = Level {
            owner_lasts: &[],
            depth: 1,
            last3: None,
            owner_top: None,
        };
        self.emit_list(&mut replies, main, thread, root);
        if replies
            .iter()
            .any(|s| matches!(s.kind, SlotKind::Reply { .. }))
        {
            out.push(Slot::plain(SlotKind::Rail));
        }
        out.extend(replies);
        out
    }

    /// Every id drawn in the depth-3 column starting with `item`, in drawing order.
    fn column_ids<'a>(&'a self, item: &'a ThreadItem, out: &mut Vec<&'a str>) {
        let id = item_id(item);
        out.push(id);
        if self.expanded.contains(id)
            && let Some(list) = self.children.get(id)
        {
            for child in &list.items {
                self.column_ids(child, out);
            }
        }
    }

    fn has_tail(list: Option<&ReplyList>) -> bool {
        list.is_some_and(|l| l.next_cursor.is_some() || l.pending.is_some() || l.failed().is_some())
    }

    /// Emits the rows of the list under `parent`; `depth` is the level of its items (4 and deeper are drawn flat).
    fn emit_list(&self, out: &mut Vec<Slot>, main: MainInfo, parent: &str, level: Level) {
        let Level {
            owner_lasts,
            depth,
            last3,
            owner_top,
        } = level;
        let thread_id = main.id;
        let first_level = parent == thread_id;
        let list = self.children.get(parent);
        let items: Vec<&ThreadItem> = if first_level {
            self.first_level(thread_id)
        } else {
            list.map(|l| l.items.iter().collect()).unwrap_or_default()
        };
        let tail = Self::has_tail(list);
        let mut own_last3: Option<String> = None;
        if depth == 3 {
            let mut ids = Vec::new();
            for item in &items {
                self.column_ids(item, &mut ids);
            }
            if !tail {
                own_last3 = ids.last().map(|s| s.to_string());
            }
        }
        let last3 = if depth == 3 {
            own_last3.as_deref()
        } else {
            last3
        };
        for (i, item) in items.iter().enumerate() {
            let id = item_id(item);
            let is_last = if depth >= 3 {
                last3 == Some(id)
            } else {
                i + 1 == items.len() && !tail
            };
            let lasts: Vec<bool> = if depth <= 3 {
                let mut v = owner_lasts.to_vec();
                v.push(is_last);
                v
            } else {
                let mut v = owner_lasts[..2].to_vec();
                v.push(is_last);
                v
            };
            let top = if first_level { Some(i) } else { owner_top };
            let expanded = self.expanded.contains(id);
            let child_list = self.children.get(id);
            let replies = item_replies(item);
            let has_children = if expanded {
                child_list.is_some_and(|l| !l.items.is_empty()) || Self::has_tail(child_list)
            } else {
                replies > 0
            };
            let quote = first_level
                && matches!(item, ThreadItem::Commit(c)
                    if c.parent_id.as_deref() != Some(thread_id)
                        && c.quoted_commit_id.as_deref() == Some(thread_id));
            let replying_to = if depth >= 4 {
                self.author_of(parent)
            } else {
                None
            };
            let parent_key = parent.to_string();
            out.push(Slot {
                kind: SlotKind::Reply {
                    item: (*item).clone(),
                    lasts: lasts.clone(),
                    replying_to,
                    quote,
                    has_children,
                },
                entry: Some(Entry::Reply(id.to_string(), parent_key)),
                top,
            });
            if expanded {
                let below = Level {
                    owner_lasts: &lasts,
                    depth: depth + 1,
                    last3,
                    owner_top: top,
                };
                self.emit_list(out, main, id, below);
            } else if replies > 0 {
                out.push(Slot {
                    kind: SlotKind::Hint {
                        lasts: lasts.clone(),
                        count: replies,
                    },
                    entry: None,
                    top,
                });
            }
        }
        self.emit_tail(out, main, parent, owner_lasts, items.len(), owner_top);
    }

    fn author_of(&self, id: &str) -> Option<String> {
        let find = |items: &[ThreadItem]| {
            items.iter().find_map(|i| match i {
                ThreadItem::Commit(c) if c.id == id => Some(c.author.username.clone()),
                _ => None,
            })
        };
        self.children.values().find_map(|l| find(&l.items))
    }

    fn emit_tail(
        &self,
        out: &mut Vec<Slot>,
        main: MainInfo,
        parent: &str,
        lasts: &[bool],
        shown: usize,
        owner_top: Option<usize>,
    ) {
        let Some(list) = self.children.get(parent) else {
            return;
        };
        let first_level = parent == main.id;
        let top = if first_level { Some(shown) } else { owner_top };
        let lasts = lasts.to_vec();
        if list.next_cursor.is_some() {
            let text = if list.pending.is_some() {
                if first_level {
                    "(loading more...)".to_string()
                } else {
                    "(loading...)".to_string()
                }
            } else if first_level {
                let missing = i64::from(main.replies) - shown as i64;
                if missing > 0 {
                    format!("Show {missing} more")
                } else {
                    "Show more".to_string()
                }
            } else {
                "Show more (o)".to_string()
            };
            out.push(Slot {
                kind: SlotKind::Note {
                    lasts: lasts.clone(),
                    text,
                },
                entry: Some(Entry::ShowMore(parent.to_string())),
                top,
            });
        } else if list.pending.is_some() {
            let text = if first_level && shown > 0 {
                "(loading more...)"
            } else {
                "(loading...)"
            };
            out.push(Slot {
                kind: SlotKind::Note {
                    lasts: lasts.clone(),
                    text: text.to_string(),
                },
                entry: None,
                top,
            });
        }
        if list.pending.is_none()
            && let Some(code) = list.failed()
        {
            out.push(Slot {
                kind: SlotKind::Error {
                    lasts,
                    code: code.to_string(),
                    retry_key: if first_level { 'R' } else { 'o' },
                },
                entry: None,
                top,
            });
        }
    }
}
