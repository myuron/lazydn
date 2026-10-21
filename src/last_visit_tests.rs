use crate::action::Action;
use crate::app::{App, LogTab};
use crate::feed::FeedPage;
use crate::feed_tests::{FOLLOWING, LATEST, TRENDING, deliver, started};
use crate::model::{Commit, LogPage};
use crate::test_support::*;

const LOG: &str = include_str!("../tests/fixtures/log_following.json");

/// `n` commits made a minute apart on one day, ids `v0`, `v1`, ...
fn page(n: usize) -> FeedPage {
    let first: Commit = serde_json::from_str::<LogPage>(LOG).unwrap().data.remove(0);
    let items = (0..n)
        .map(|i| Commit {
            id: format!("v{i}"),
            created_at: NOW - time::Duration::minutes(i as i64 + 1),
            ..first.clone()
        })
        .collect();
    FeedPage {
        items,
        next_cursor: None,
        is_ranked: false,
        suggestions: Vec::new(),
    }
}

fn scroll_at_last(app: &mut App) -> u16 {
    app.update(Action::SelectLast);
    app.feed(FOLLOWING).scroll
}

#[test]
fn set_last_visit_is_kept_per_tab() {
    let mut app = signed_in(120, 40);
    app.update(Action::SetLastVisit {
        tab: LogTab::Following,
        id: "f1".into(),
    });
    app.update(Action::SetLastVisit {
        tab: LogTab::Latest,
        id: "l1".into(),
    });
    assert_eq!(app.last_visit_of(LogTab::Following), Some("f1"));
    assert_eq!(app.last_visit_of(LogTab::Latest), Some("l1"));
    assert_eq!(app.last_visit_of(LogTab::Trending), None);
}

#[test]
fn set_last_visit_asks_for_no_request() {
    let mut app = signed_in(120, 40);
    let effects = app.update(Action::SetLastVisit {
        tab: LogTab::Following,
        id: "f1".into(),
    });
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn set_last_visit_settles_the_scroll_for_the_row_it_adds() {
    let (mut plain, effects) = started(100, 24);
    deliver(&mut plain, FOLLOWING, &effects, page(12));
    let without_marker = scroll_at_last(&mut plain);

    let (mut expected, effects) = started(100, 24);
    expected.update(Action::SetLastVisit {
        tab: LogTab::Following,
        id: "v3".into(),
    });
    deliver(&mut expected, FOLLOWING, &effects, page(12));
    let with_marker = scroll_at_last(&mut expected);
    assert!(
        with_marker > without_marker,
        "marker must move the scroll: {with_marker} vs {without_marker}"
    );

    let (mut app, effects) = started(100, 24);
    deliver(&mut app, FOLLOWING, &effects, page(12));
    app.update(Action::SelectLast);
    app.update(Action::SetLastVisit {
        tab: LogTab::Following,
        id: "v3".into(),
    });
    assert_eq!(app.last_visit_of(LogTab::Following), Some("v3"));
    assert_eq!(app.feed(FOLLOWING).scroll, with_marker);
}

#[test]
fn heads_to_save_has_the_first_commit_of_each_loaded_tab() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(3));
    let effects = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &effects, page(2));
    let heads = app.heads_to_save();
    assert_eq!(heads.len(), 2);
    assert_eq!(heads[&LogTab::Following], "v0");
    assert_eq!(heads[&LogTab::Latest], "v0");
}

#[test]
fn heads_to_save_leaves_out_a_tab_that_never_loaded() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(3));
    let heads = app.heads_to_save();
    assert_eq!(heads.len(), 1);
    assert!(heads.contains_key(&LogTab::Following));
}

#[test]
fn heads_to_save_leaves_out_a_tab_that_loaded_empty() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0));
    assert!(app.heads_to_save().is_empty());
}

#[test]
fn heads_to_save_never_includes_trending() {
    let (mut app, _) = started(120, 40);
    let effects = type_str(&mut app, "3");
    deliver(&mut app, TRENDING, &effects, page(3));
    assert!(app.heads_to_save().is_empty());
}

// --- the HEAD marker ----------------------------------------------------------

fn drawn(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| row_text(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn visit(app: &mut App, tab: LogTab, id: &str) {
    app.update(Action::SetLastVisit { tab, id: id.into() });
}

#[test]
fn the_head_marker_shows_on_the_tab_whose_visit_matches() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(5));
    visit(&mut app, LogTab::Following, "v2");
    assert!(drawn(&app).contains("HEAD@"));
}

#[test]
fn the_head_marker_does_not_show_for_another_tabs_visit() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(5));
    visit(&mut app, LogTab::Following, "zzz");
    visit(&mut app, LogTab::Latest, "v2");
    assert!(!drawn(&app).contains("HEAD@"));
}

#[test]
fn the_head_marker_does_not_show_on_latest_for_followings_visit() {
    let (mut app, _) = started(120, 40);
    visit(&mut app, LogTab::Following, "v2");
    let effects = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &effects, page(5));
    assert!(!drawn(&app).contains("HEAD@"));
}

#[test]
fn the_head_marker_does_not_show_on_trending() {
    let (mut app, _) = started(120, 40);
    for tab in [LogTab::Following, LogTab::Latest, LogTab::Trending] {
        visit(&mut app, tab, "v2");
    }
    let effects = type_str(&mut app, "3");
    let mut ranked = page(5);
    ranked.is_ranked = true;
    deliver(&mut app, TRENDING, &effects, ranked);
    assert!(!drawn(&app).contains("HEAD@"));
}

#[test]
fn the_head_marker_does_not_show_in_a_profile_list() {
    let mut app = crate::profile_tests::alice(120, 40, 4);
    for tab in [LogTab::Following, LogTab::Latest, LogTab::Trending] {
        visit(&mut app, tab, "p2");
    }
    assert!(!drawn(&app).contains("HEAD@"));
}
