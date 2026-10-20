use super::writes::WriteKind;
use super::{App, Modal, ScreenKind, Session};
use crate::action::Effect;
use crate::dialogs::{MenuItem, MoreMenu};
use crate::keymap::MenuKey;
use crate::model::{Commit, ThreadItem};

/// The more menu of a card.
impl App {
    /// The open more menu.
    pub fn menu(&self) -> Option<&MoreMenu> {
        self.menu.as_ref()
    }

    /// The items that apply to the card, in the order of the docs.
    fn menu_items(&self, card: &Commit) -> Vec<MenuItem> {
        let own = !self.not_own(card);
        let pastel = matches!(&self.session, Session::SignedIn(me) if me.pasteis_mode) && !own;
        let item = |key, label| MenuItem { key, label };
        let flag = |on: Option<bool>, yes, no| if on == Some(true) { yes } else { no };
        [
            (!own).then(|| item('s', flag(card.starred, "Unstar", "Star"))),
            pastel.then(|| item('p', "Give a pastel")),
            Some(item('b', flag(card.is_saved, "Unsave", "Save"))),
            Some(item('y', "Copy link")),
            Some(item('O', "Open in browser")),
            (!own).then(|| item('!', "Report")),
            own.then(|| item('d', "Revert commit")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub(super) fn open_menu(&mut self) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        let Some(card) = self.selected_commit().filter(|c| !self.is_deleting(&c.id)) else {
            return Vec::new();
        };
        self.menu = Some(MoreMenu {
            items: self.menu_items(&card),
            id: card.id,
            short_hash: card.short_hash,
            selected: 0,
        });
        self.modal = Some(Modal::Menu);
        Vec::new()
    }

    pub(super) fn on_menu_key(&mut self, key: MenuKey) -> Vec<Effect> {
        let Some(menu) = self.menu.as_mut() else {
            return Vec::new();
        };
        match key {
            MenuKey::Up => menu.selected = menu.selected.saturating_sub(1),
            MenuKey::Down => {
                menu.selected = (menu.selected + 1).min(menu.items.len().saturating_sub(1));
            }
            MenuKey::Select => {
                let chosen = menu.items.get(menu.selected).map(|i| i.key);
                return chosen.map_or_else(Vec::new, |k| self.run_menu_item(k));
            }
            MenuKey::Item(c) if menu.items.iter().any(|i| i.key == c) => {
                return self.run_menu_item(c);
            }
            MenuKey::Item(_) => {}
        }
        Vec::new()
    }

    /// Runs a listed item on the card the menu was opened on, whatever the list holds under the selection now.
    fn run_menu_item(&mut self, key: char) -> Vec<Effect> {
        let Some(menu) = self.menu.take() else {
            return Vec::new();
        };
        let id = menu.id;
        self.modal = None;
        let Some(card) = self.commit_on_screen(&id) else {
            return Vec::new();
        };
        match key {
            's' => self.toggle_on(WriteKind::Star, card),
            'p' => self.give_pastel_to(card),
            'b' => self.toggle_on(WriteKind::Bookmark, card),
            'y' => Self::copy_link_of(&card),
            'O' => Self::open_link_of(&card),
            '!' => self.open_report_on(card),
            'd' => self.open_delete_on(card),
            _ => Vec::new(),
        }
    }

    /// The commit as a card of the screen, if the screen still shows it as one.
    fn commit_on_screen(&self, id: &str) -> Option<Commit> {
        let as_card = |item: &ThreadItem| match item {
            ThreadItem::Commit(c) if c.id == id => Some((**c).clone()),
            _ => None,
        };
        match self.kind() {
            ScreenKind::Feed(key) => self
                .feeds
                .get(&key)?
                .items
                .iter()
                .find(|c| c.id == id)
                .cloned(),
            ScreenKind::Profile(name) => {
                let state = self.profile(&name).filter(|s| s.shows_list())?;
                state
                    .cards(self.signed_in())
                    .into_iter()
                    .find(|c| c.id == id)
                    .cloned()
            }
            ScreenKind::Thread(thread) => {
                let state = self.threads.get(&thread)?;
                state
                    .main_commit()
                    .filter(|c| c.id == id)
                    .cloned()
                    .or_else(|| {
                        let listed = state.children.values().flat_map(|l| l.items.iter());
                        state.ancestors.iter().chain(listed).find_map(as_card)
                    })
            }
            ScreenKind::Connections(..) | ScreenKind::Notifications | ScreenKind::Settings => None,
        }
    }
}
