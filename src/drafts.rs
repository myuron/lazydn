//! Unsent text kept per account: what the composer holds, and the file it is saved to.
//!
//! The file is `drafts-<account id>.toml` in the state directory, one `[[draft]]` table per entry, oldest first:
//!
//! ```toml
//! [[draft]]
//! kind = "new"      # or "reply" / "quote"
//! id = "c1"         # the commit replied to or quoted; absent for "new"
//! text = "..."
//! ```

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::editor::Editor;

/// Most reply and quote drafts kept; the oldest go first.
pub const REPLY_QUOTE_CAP: usize = 50;

/// A file larger than this is not read: it was not written by lazydn.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Which unsent text a composer edits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DraftKey {
    New,
    /// The id of the commit replied to.
    Reply(String),
    /// The id of the commit quoted.
    Quote(String),
}

/// The drafts of the signed-in account, oldest first; an empty editor is never kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drafts {
    entries: Vec<(DraftKey, Editor)>,
}

impl Drafts {
    /// The editor of `key`, if it holds text.
    pub fn get(&self, key: &DraftKey) -> Option<&Editor> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, e)| e)
    }

    /// The text of `key`, if it holds any.
    pub fn text(&self, key: &DraftKey) -> Option<String> {
        self.get(key).map(Editor::text)
    }

    /// Stores `editor` as the newest draft of `key`; an empty editor removes the draft. Keeps at most
    /// [`REPLY_QUOTE_CAP`] reply and quote drafts, dropping the oldest.
    pub fn set(&mut self, key: DraftKey, editor: Editor) {
        self.remove(&key);
        if editor.is_empty() {
            return;
        }
        self.entries.push((key, editor));
        self.enforce_cap();
    }

    /// Adds the drafts of `loaded`, which are older than anything here, into the slots that hold no text; the
    /// oldest reply and quote drafts go first when the cap is passed.
    pub fn fill_missing(&mut self, loaded: Drafts) {
        let mut merged: Vec<(DraftKey, Editor)> = loaded
            .entries
            .into_iter()
            .filter(|(key, _)| self.get(key).is_none())
            .collect();
        merged.append(&mut self.entries);
        self.entries = merged;
        self.enforce_cap();
    }

    pub fn remove(&mut self, key: &DraftKey) {
        self.entries.retain(|(k, _)| k != key);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The keys, oldest first.
    pub fn keys(&self) -> Vec<DraftKey> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    fn enforce_cap(&mut self) {
        let counted = |k: &DraftKey| !matches!(k, DraftKey::New);
        while self.entries.iter().filter(|(k, _)| counted(k)).count() > REPLY_QUOTE_CAP {
            if let Some(oldest) = self.entries.iter().position(|(k, _)| counted(k)) {
                self.entries.remove(oldest);
            }
        }
    }
}

/// Whether `account` may name a file: 1 to 64 characters of lowercase hex digits and hyphens.
pub fn is_valid_account_id(account: &str) -> bool {
    (1..=64).contains(&account.len())
        && account
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

/// The file of `account`'s drafts; `None` when the id is not valid.
pub fn drafts_path(state_dir: &Path, account: &str) -> Option<PathBuf> {
    is_valid_account_id(account).then(|| state_dir.join(format!("drafts-{account}.toml")))
}

/// Reads the drafts of `account`. A missing, corrupt, oversized or unreadable file, or an invalid id, gives no
/// drafts. Unsafe characters are dropped and each text is cut at 8000 characters.
pub fn load_drafts(state_dir: &Path, account: &str) -> Drafts {
    let mut drafts = Drafts::default();
    let Some(path) = drafts_path(state_dir, account) else {
        return drafts;
    };
    // The checks and the read use one handle, so the file cannot change into something else in between.
    let Ok(file) = fs::File::open(&path) else {
        return drafts;
    };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= MAX_FILE_BYTES)
    {
        return drafts;
    }
    let mut text = String::new();
    if file
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .is_err()
        || text.len() as u64 > MAX_FILE_BYTES
    {
        return drafts;
    }
    let Ok(table) = text.parse::<toml::Table>() else {
        return drafts;
    };
    let Some(list) = table.get("draft").and_then(toml::Value::as_array) else {
        return drafts;
    };
    for entry in list {
        let field = |name: &str| entry.get(name).and_then(toml::Value::as_str);
        let key = match (field("kind"), field("id")) {
            (Some("new"), _) => DraftKey::New,
            (Some("reply"), Some(id)) if is_valid_account_id(id) => DraftKey::Reply(id.into()),
            (Some("quote"), Some(id)) if is_valid_account_id(id) => DraftKey::Quote(id.into()),
            _ => continue,
        };
        let Some(body) = field("text") else {
            continue;
        };
        let mut editor = Editor::new();
        editor.paste(body);
        drafts.set(key, editor);
    }
    drafts
}

/// Replaces the file with `drafts`; with none left the file is removed. A directory made for it is 0700 and the
/// file is 0600, and the file is replaced atomically. An invalid id is an error and writes nothing.
pub fn save_drafts(state_dir: &Path, account: &str, drafts: &Drafts) -> io::Result<()> {
    let path = drafts_path(state_dir, account)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid account id"))?;
    if drafts.is_empty() {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let list: Vec<toml::Value> = drafts
        .entries
        .iter()
        .map(|(key, editor)| {
            let mut t = toml::Table::new();
            let (kind, id) = match key {
                DraftKey::New => ("new", None),
                DraftKey::Reply(id) => ("reply", Some(id)),
                DraftKey::Quote(id) => ("quote", Some(id)),
            };
            t.insert("kind".into(), kind.into());
            if let Some(id) = id {
                t.insert("id".into(), id.as_str().into());
            }
            t.insert("text".into(), editor.text().into());
            toml::Value::Table(t)
        })
        .collect();
    let mut root = toml::Table::new();
    root.insert("draft".into(), toml::Value::Array(list));
    let text = toml::to_string(&root).map_err(io::Error::other)?;

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(state_dir)?;
    let tmp = state_dir.join(format!(".drafts-{account}.toml.tmp-{}", std::process::id()));
    // A leftover of a crashed run is removed first, so that the file is made here with the mode asked for.
    let _ = fs::remove_file(&tmp);
    let written = write_private(&tmp, &text).and_then(|()| fs::rename(&tmp, &path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// Synced before the rename so that a crash cannot leave an empty file in place of the old one.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}
