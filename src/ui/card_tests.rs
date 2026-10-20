use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::macros::datetime;
use time::{Duration, OffsetDateTime, UtcOffset};
use unicode_width::UnicodeWidthStr;

use super::card::*;
use super::theme;
use crate::highlight::HlClass;
use crate::lane::{auto_lane, rgb, to_ansi256};
use crate::model::{Commit, Lane, Rgb, Tombstone};

const W: u16 = 70;
const NOW: OffsetDateTime = datetime!(2026-10-20 12:00 UTC);
const ACTIONS: &str = "☆ 12  ◓ 3  ❝ Quote  ↩ Reply  ⌄ 2 replies";
const TOMBSTONE_TEXT: &str = "[deleted] This commit was deleted.";

// --- fixtures ---------------------------------------------------------------

fn ctx() -> RenderCtx {
    RenderCtx {
        now: NOW,
        local_offset: UtcOffset::UTC,
        truecolor: true,
        ascii: false,
        pasteis_mode: true,
        reply_preview_limit: 3,
    }
}

fn at(delta: Duration) -> String {
    (NOW + delta).format(&Rfc3339).unwrap()
}

fn merge(mut base: Value, extra: Value) -> Value {
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    base
}

fn author_json(username: &str, name: Option<&str>, lane: Option<u8>) -> Value {
    json!({
        "id": format!("user-{username}"),
        "username": username,
        "name": name,
        "created_at": "2026-10-01T00:00:00Z",
        "lane": lane,
    })
}

/// Signed-in commit by Alice, 3 minutes old, with every signed-only field present.
fn commit_json(extra: Value) -> Value {
    merge(
        json!({
            "id": "c1",
            "short_hash": "a1b2c3d",
            "body": "Hello",
            "parent_id": null,
            "html_url": null,
            "created_at": at(Duration::minutes(-3)),
            "stars_count": 12,
            "replies_count": 2,
            "link_preview": null,
            "images": [],
            "author": author_json("alice", Some("Alice"), Some(1)),
            "body_segments": [text("Hello")],
            "preview_segments": null,
            "mentions": [],
            "status": "approved",
            "is_pinned": false,
            "starred": false,
            "is_saved": false,
            "pasteis_count": 3,
            "quoted_commit_id": null,
        }),
        extra,
    )
}

fn commit(extra: Value) -> Commit {
    serde_json::from_value(commit_json(extra)).unwrap()
}

/// Public-API shape: no segments and none of the viewer-specific fields.
fn guest_commit(extra: Value) -> Commit {
    commit(merge(
        json!({
            "body_segments": null,
            "preview_segments": null,
            "mentions": null,
            "status": null,
            "is_pinned": null,
            "starred": null,
            "is_saved": null,
            "pasteis_count": null,
        }),
        extra,
    ))
}

fn text(s: &str) -> Value {
    json!({"type": "text", "text": s})
}

fn code(s: &str) -> Value {
    json!({"type": "code", "code": s})
}

fn block(language: Option<&str>, html: &str) -> Value {
    json!({"type": "block", "language": language, "html": html})
}

fn reply_json(username: &str, name: &str, lane: u8, body: &str, ago_min: i64) -> Value {
    commit_json(json!({
        "id": format!("r-{username}"),
        "short_hash": "000c0c0",
        "body": body,
        "created_at": at(Duration::minutes(-ago_min)),
        "replies_count": 0,
        "author": author_json(username, Some(name), Some(lane)),
        "body_segments": null,
    }))
}

fn bob() -> Value {
    reply_json("bob", "Bob", 3, "First reply", 1)
}

fn carol() -> Value {
    reply_json("carol", "Carol", 4, "Third reply", 2)
}

fn tombstone_json() -> Value {
    json!({"id": "t1", "parent_id": "c1", "deleted_at": at(Duration::hours(-1)), "replies_count": 0})
}

fn quoted_json(body: &str) -> Value {
    commit_json(json!({
        "id": "q1",
        "short_hash": "0000q1q",
        "body": body,
        "created_at": at(Duration::hours(-2)),
        "author": author_json("bob", Some("Bob"), Some(3)),
        "body_segments": null,
    }))
}

fn with_replies(items: Vec<Value>, replies_count: u32, next_cursor: Option<&str>) -> Commit {
    commit(json!({
        "replies_count": replies_count,
        "reply_preview": {"data": items, "next_cursor": next_cursor},
    }))
}

fn poll_json(
    closes_at: Option<String>,
    is_closed: bool,
    total_votes: Option<u32>,
    options: &[(&str, u8)],
) -> Value {
    let options: Vec<Value> = options
        .iter()
        .enumerate()
        .map(|(i, (label, pct))| {
            json!({
                "id": format!("o{i}"),
                "label": label,
                "votes_count": total_votes.map(|_| 1),
                "voted": false,
                "percentage": total_votes.map(|_| *pct),
            })
        })
        .collect();
    json!({
        "id": "p1",
        "closes_at": closes_at,
        "is_closed": is_closed,
        "has_voted": false,
        "total_votes": total_votes,
        "options": options,
    })
}

fn link_json(extra: Value) -> Value {
    merge(
        json!({
            "url": "https://example.com/path?q=1",
            "title": "Example Title",
            "description": "An example description.",
            "image_url": null,
            "site_name": "Example Site",
            "is_pending": false,
            "embed": null,
        }),
        extra,
    )
}

fn image_json(w: u32, h: u32, description: Option<&str>) -> Value {
    json!({"id": format!("i{w}"), "width": w, "height": h, "description": description, "url": "https://example.com/i.png"})
}

fn tombstone() -> Tombstone {
    serde_json::from_value(tombstone_json()).unwrap()
}

fn opts() -> CardOptions {
    CardOptions::default()
}

fn thread_main() -> CardOptions {
    CardOptions {
        variant: Variant::ThreadMain,
        ..opts()
    }
}

fn prefix() -> Prefix {
    Prefix {
        first: vec![Span::raw("├ ")],
        rest: vec![Span::raw("│ ")],
    }
}

fn text_of_len(n: usize) -> String {
    (0..n)
        .map(|i| if i % 10 == 9 && i + 1 < n { ' ' } else { 'a' })
        .collect()
}

// --- rendering helpers ------------------------------------------------------

fn lines_of(c: &Commit, o: &CardOptions, ctx: &RenderCtx, w: u16) -> Vec<Line<'static>> {
    card_lines(CardInput::Commit(c), o, ctx, w, &Prefix::default())
}

fn plain(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn texts(lines: &[Line]) -> Vec<String> {
    lines.iter().map(plain).collect()
}

fn card_text(c: &Commit, o: &CardOptions, ctx: &RenderCtx, w: u16) -> Vec<String> {
    texts(&lines_of(c, o, ctx, w))
}

fn terminal_of(lines: Vec<Line<'static>>, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| f.render_widget(Paragraph::new(lines), f.area()))
        .unwrap();
    terminal
}

fn draw_commit(
    c: &Commit,
    o: &CardOptions,
    ctx: &RenderCtx,
    w: u16,
    h: u16,
) -> Terminal<TestBackend> {
    terminal_of(lines_of(c, o, ctx, w), w, h)
}

fn buffer_of(lines: Vec<Line<'static>>, w: u16, h: u16) -> Buffer {
    terminal_of(lines, w, h).backend().buffer().clone()
}

fn find_symbol(buffer: &Buffer, symbol: &str) -> Option<(u16, u16)> {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer[(x, y)].symbol() == symbol {
                return Some((x, y));
            }
        }
    }
    None
}

fn pad_between(prefix: &str, left: &str, right: &str, w: u16) -> String {
    let used = prefix.width() + left.width() + right.width();
    format!("{prefix}{left}{}{right}", " ".repeat(usize::from(w) - used))
}

fn hdr(left: &str) -> String {
    pad_between(" ◉ ", left, "a1b2c3d", W)
}

fn act(left: &str) -> String {
    pad_between(" │ ", left, "⊡ ⚑", W)
}

fn l(s: &str) -> String {
    format!(" │ {s}")
}

/// Everything between the header and the action row of a card without reply preview.
fn middle(t: &[String]) -> &[String] {
    assert!(t.len() >= 2, "card has no header and action row: {t:?}");
    &t[1..t.len() - 1]
}

fn framed_with(glyphs: [&str; 6], width: usize, title: Option<&str>, body: &[&str]) -> Vec<String> {
    let [corner_tl, h, v, corner_tr, corner_bl, corner_br] = glyphs;
    let top = match title {
        Some(t) => format!("{corner_tl} {t} "),
        None => corner_tl.to_string(),
    };
    let fill = width - top.width() - 1;
    let mut out = vec![format!("{top}{}{corner_tr}", h.repeat(fill))];
    for b in body {
        out.push(format!("{v} {b}{} {v}", " ".repeat(width - 4 - b.width())));
    }
    out.push(format!("{corner_bl}{}{corner_br}", h.repeat(width - 2)));
    out
}

const SOLID: [&str; 6] = ["┌", "─", "│", "┐", "└", "┘"];
const DOTTED: [&str; 6] = ["┌", "╌", "╎", "┐", "└", "┘"];
const ASCII_SOLID: [&str; 6] = ["+", "-", "|", "+", "+", "+"];
const ASCII_DOTTED: [&str; 6] = ["+", ".", ":", "+", "+", "+"];

fn solid_box(title: Option<&str>, body: &[&str]) -> Vec<String> {
    framed_with(SOLID, 67, title, body)
        .iter()
        .map(|s| l(s))
        .collect()
}

fn dotted_box(body: &[&str]) -> Vec<String> {
    framed_with(DOTTED, 67, None, body)
        .iter()
        .map(|s| l(s))
        .collect()
}

/// Positional access that fails as an assertion, so an empty card reports what is missing.
fn row(t: &[String], i: usize) -> &String {
    assert!(t.len() > i, "expected a row at index {i}, got {t:#?}");
    &t[i]
}

fn rows_from(t: &[String], i: usize) -> &[String] {
    assert!(t.len() >= i, "expected at least {i} rows, got {t:#?}");
    &t[i..]
}

macro_rules! snapshot {
    ($name:literal, $terminal:expr) => {{
        let terminal = $terminal;
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

// --- 1. folding -------------------------------------------------------------

#[test]
fn t01_list_with_preview_segments_shows_preview_then_show_more() {
    let c = commit(json!({
        "body_segments": [text("The full body that the preview cuts off")],
        "preview_segments": [text("Preview…")],
    }));
    assert_eq!(
        card_text(&c, &opts(), &ctx(), W),
        vec![
            hdr("Alice @alice · 3m"),
            l("Preview…"),
            l("Show more"),
            act(ACTIONS)
        ]
    );
}

#[test]
fn t01_thread_main_shows_every_body_segment_without_show_more() {
    let c = commit(json!({
        "body_segments": [
            text("Full body"),
            block(Some("rust"), "fn main() {}"),
            text("tail"),
        ],
        "preview_segments": [text("Preview…")],
    }));
    let t = card_text(&c, &thread_main(), &ctx(), W);
    assert!(t.contains(&l("Full body")), "{t:#?}");
    assert!(t.contains(&l("tail")), "{t:#?}");
    assert!(t.iter().all(|s| !s.contains("Show more")), "{t:#?}");
    assert!(t.iter().all(|s| !s.contains("Preview")), "{t:#?}");
}

#[test]
fn t01_guest_body_of_281_chars_is_folded_but_280_is_not() {
    let folded = card_text(
        &guest_commit(json!({"body": text_of_len(281)})),
        &opts(),
        &ctx(),
        W,
    );
    assert!(folded.contains(&l("Show more")), "{folded:#?}");
    assert!(
        folded.iter().any(|s| s.contains('…')),
        "folded body ends with an ellipsis: {folded:#?}"
    );

    let whole = card_text(
        &guest_commit(json!({"body": text_of_len(280)})),
        &opts(),
        &ctx(),
        W,
    );
    assert!(whole.iter().any(|s| s.contains("aaaaaaaaa")), "{whole:#?}");
    assert!(whole.iter().all(|s| !s.contains("Show more")), "{whole:#?}");
    assert!(whole.iter().all(|s| !s.contains('…')), "{whole:#?}");
}

#[test]
fn t01_guest_body_of_9_lines_is_folded_at_8_lines_but_8_lines_is_not() {
    let numbered = |n: usize| {
        (1..=n)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let folded = card_text(
        &guest_commit(json!({"body": numbered(9)})),
        &opts(),
        &ctx(),
        W,
    );
    assert_eq!(
        middle(&folded),
        &[
            l("line1"),
            l("line2"),
            l("line3"),
            l("line4"),
            l("line5"),
            l("line6"),
            l("line7"),
            l("line8…"),
            l("Show more"),
        ]
    );

    let whole = card_text(
        &guest_commit(json!({"body": numbered(8)})),
        &opts(),
        &ctx(),
        W,
    );
    assert_eq!(
        middle(&whole),
        &[
            l("line1"),
            l("line2"),
            l("line3"),
            l("line4"),
            l("line5"),
            l("line6"),
            l("line7"),
            l("line8"),
        ]
    );
}

// --- 2. action row ----------------------------------------------------------

fn last(t: &[String]) -> &String {
    assert!(!t.is_empty(), "expected at least one row");
    t.last().unwrap()
}

#[test]
fn t02_pasteis_count_is_hidden_when_pasteis_mode_is_off() {
    let c = ctx();
    let c = RenderCtx {
        pasteis_mode: false,
        ..c
    };
    let t = card_text(&commit(json!({})), &opts(), &c, W);
    assert_eq!(last(&t), &act("☆ 12  ❝ Quote  ↩ Reply  ⌄ 2 replies"));
}

#[test]
fn t02_replies_label_is_absent_for_0_singular_for_1_plural_for_2() {
    let row = |count: u32| {
        let t = card_text(&commit(json!({"replies_count": count})), &opts(), &ctx(), W);
        last(&t).clone()
    };
    assert_eq!(row(0), act("☆ 12  ◓ 3  ❝ Quote  ↩ Reply"));
    assert_eq!(row(1), act("☆ 12  ◓ 3  ❝ Quote  ↩ Reply  ⌄ 1 reply"));
    assert_eq!(row(2), act("☆ 12  ◓ 3  ❝ Quote  ↩ Reply  ⌄ 2 replies"));
}

#[test]
fn t02_draft_replaces_reply_label_with_stash() {
    let o = CardOptions {
        has_draft: true,
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ctx(), W);
    assert_eq!(
        last(&t),
        &act("☆ 12  ◓ 3  ❝ Quote  ↩ stash@{0}  ⌄ 2 replies")
    );
}

#[test]
fn t02_starred_and_saved_use_filled_icons() {
    let t = card_text(
        &commit(json!({"starred": true, "is_saved": true})),
        &opts(),
        &ctx(),
        W,
    );
    assert_eq!(
        last(&t),
        &pad_between(" │ ", "★ 12  ◓ 3  ❝ Quote  ↩ Reply  ⌄ 2 replies", "⊠ ⚑", W)
    );
}

#[test]
fn t02_missing_counts_are_shown_as_zero() {
    let c = guest_commit(json!({"stars_count": null, "replies_count": null}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(last(&t), &act("☆ 0  ◓ 0  ❝ Quote  ↩ Reply"));
}

// --- 3. header --------------------------------------------------------------

#[test]
fn t03_missing_name_falls_back_to_username() {
    let c = commit(json!({"author": author_json("alice", None, Some(1))}));
    assert_eq!(
        row(&card_text(&c, &opts(), &ctx(), W), 0),
        &hdr("alice @alice · 3m")
    );
}

#[test]
fn t03_narrow_width_truncates_the_name_and_keeps_the_hash_at_the_right_edge() {
    let c = commit(json!({"author": author_json("alice", Some("Alexander the Great"), Some(1))}));
    let header = row(&card_text(&c, &opts(), &ctx(), 30), 0).clone();
    assert_eq!(header, " ◉ Alexan… @alice · 3m a1b2c3d");
    assert_eq!(header.width(), 30);
}

#[test]
fn t03_own_pending_and_rejected_commits_get_badges() {
    let own = CardOptions {
        is_own: true,
        ..opts()
    };
    let header = |status: &str, o: &CardOptions| {
        row(
            &card_text(&commit(json!({"status": status})), o, &ctx(), W),
            0,
        )
        .clone()
    };
    assert_eq!(
        header("pending", &own),
        hdr("Alice @alice · 3m  [in review]")
    );
    assert_eq!(
        header("rejected", &own),
        hdr("Alice @alice · 3m  [rejected]")
    );
    assert_eq!(header("approved", &own), hdr("Alice @alice · 3m"));
    assert_eq!(header("pending", &opts()), hdr("Alice @alice · 3m"));
    assert_eq!(header("rejected", &opts()), hdr("Alice @alice · 3m"));
}

#[test]
fn t03_thread_position_follows_the_time_and_precedes_the_badge() {
    let o = CardOptions {
        is_own: true,
        thread_pos: Some((1, 2)),
        ..opts()
    };
    let t = card_text(&commit(json!({"status": "pending"})), &o, &ctx(), W);
    assert_eq!(row(&t, 0), &hdr("Alice @alice · 3m  [1/2] [in review]"));
}

#[test]
fn t03_thread_main_header_has_no_time_and_no_hash() {
    let o = CardOptions {
        thread_pos: Some((1, 2)),
        ..thread_main()
    };
    let t = card_text(&commit(json!({})), &o, &ctx(), W);
    assert_eq!(row(&t, 0), &" ◉ Alice @alice  [1/2]");
}

#[test]
fn t03_same_author_header_has_name_and_position_only() {
    let o = CardOptions {
        node: Node::SameAuthor,
        thread_pos: Some((2, 2)),
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ctx(), W);
    assert_eq!(
        row(&t, 0),
        &pad_between(" • ", "Alice  [2/2]", "a1b2c3d", W)
    );
}

// --- 4. quote ---------------------------------------------------------------

#[test]
fn t04_quote_not_loaded_draws_no_box() {
    let t = card_text(&commit(json!({})), &opts(), &ctx(), W);
    assert_eq!(middle(&t), &[l("Hello")]);
}

#[test]
fn t04_quote_unavailable_draws_dotted_box() {
    let t = card_text(&commit(json!({"quoted_commit": null})), &opts(), &ctx(), W);
    let mut want = vec![l("Hello")];
    want.extend(dotted_box(&["[unavailable] This commit isn't available."]));
    assert_eq!(middle(&t), want);
}

#[test]
fn t04_quote_of_tombstone_draws_dotted_deleted_box() {
    let t = card_text(
        &commit(json!({"quoted_commit": tombstone_json()})),
        &opts(),
        &ctx(),
        W,
    );
    let mut want = vec![l("Hello")];
    want.extend(dotted_box(&[TOMBSTONE_TEXT]));
    assert_eq!(middle(&t), want);
}

#[test]
fn t04_quote_of_commit_draws_titled_box_with_excerpt() {
    let t = card_text(
        &commit(json!({"quoted_commit": quoted_json("Original thought\nsecond line")})),
        &opts(),
        &ctx(),
        W,
    );
    let mut want = vec![l("Hello")];
    want.extend(solid_box(
        Some("❝ Bob @bob · 2h"),
        &["Original thought", "second line"],
    ));
    assert_eq!(middle(&t), want);
}

#[test]
fn t04_quote_excerpt_is_limited_to_3_lines_with_ellipsis() {
    let t = card_text(
        &commit(json!({"quoted_commit": quoted_json("a\nb\nc\nd")})),
        &opts(),
        &ctx(),
        W,
    );
    let mut want = vec![l("Hello")];
    want.extend(solid_box(Some("❝ Bob @bob · 2h"), &["a", "b", "c…"]));
    assert_eq!(middle(&t), want);
}

// --- 5. reply preview -------------------------------------------------------

fn expanded() -> CardOptions {
    CardOptions {
        replies_expanded: true,
        ..opts()
    }
}

#[test]
fn t05_collapsed_card_draws_no_reply_preview() {
    let c = with_replies(vec![bob(), carol()], 2, None);
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(t, vec![hdr("Alice @alice · 3m"), l("Hello"), act(ACTIONS)]);
}

#[test]
fn t05_expanded_card_draws_up_to_limit_with_tombstone_and_last_branch() {
    let c = with_replies(vec![bob(), tombstone_json(), carol()], 5, None);
    let two = RenderCtx {
        reply_preview_limit: 2,
        ..ctx()
    };
    let t = card_text(&c, &expanded(), &two, W);
    assert_eq!(
        rows_from(&t, 3),
        [
            l("├─● Bob @bob · 1m"),
            l("│     First reply"),
            l(&format!("└─ ◌ {TOMBSTONE_TEXT}")),
            l("Show 3 more"),
        ]
    );
}

#[test]
fn t05_remaining_count_zero_with_cursor_offers_more_replies() {
    let c = with_replies(vec![bob(), carol()], 2, Some("next"));
    let t = card_text(&c, &expanded(), &ctx(), W);
    assert_eq!(
        rows_from(&t, 3),
        [
            l("├─● Bob @bob · 1m"),
            l("│     First reply"),
            l("└─● Carol @carol · 2m"),
            l("      Third reply"),
            l("Show more replies"),
        ]
    );
}

#[test]
fn t05_remaining_count_zero_without_cursor_draws_no_footer() {
    let c = with_replies(vec![bob(), carol()], 2, None);
    let t = card_text(&c, &expanded(), &ctx(), W);
    assert_eq!(last(&t), &l("      Third reply"));
}

#[test]
fn t05_replies_count_below_shown_count_does_not_panic_or_offer_more() {
    let c = with_replies(vec![bob(), tombstone_json(), carol()], 1, None);
    let t = card_text(&c, &expanded(), &ctx(), W);
    assert_eq!(t.len(), 3 + 2 + 1 + 2);
    assert_eq!(last(&t), &l("      Third reply"));
}

// --- 6. widths and prefix ---------------------------------------------------

fn everything() -> Commit {
    commit(json!({
        "author": author_json("alice", Some("日本語🥵Alice"), Some(1)),
        "status": "pending",
        "replies_count": 9,
        "body_segments": [
            text("こんにちは🥵 world と日本語の長い文章がここに続きます。"),
            code("コード🥵"),
            text(" very_long_unbroken_word_that_exceeds_twenty_columns_for_sure"),
            block(Some("rust"), "<span class=\"hl-keyword\">fn</span> 日本語🥵() { let x = \"a long line that must be truncated\"; }"),
        ],
        "images": [
            image_json(1200, 800, Some("説明🥵 long description text")),
            image_json(640, 480, None),
        ],
        "link_preview": link_json(json!({
            "title": "タイトル🥵 a very long title that needs wrapping at narrow widths",
            "description": "説明 説明 説明 a very long description that needs wrapping at narrow widths",
            "site_name": "サイト🥵",
        })),
        "poll": poll_json(
            Some(at(Duration::days(3) + Duration::hours(1))),
            false,
            Some(10),
            &[("選択肢🥵 one with a long label that wraps around", 60), ("two", 40)],
        ),
        "quoted_commit": quoted_json("引用🥵 a quoted body that is long enough to be wrapped at narrow widths"),
        "reply_preview": {
            "data": [
                reply_json("bob", "ボブ🥵", 3, "返信🥵 a reply body that is long enough to wrap", 1),
                tombstone_json(),
                carol(),
            ],
            "next_cursor": "next",
        },
    }))
}

fn busy_opts(variant: Variant, node: Node) -> CardOptions {
    CardOptions {
        variant,
        node,
        thread_pos: Some((12, 34)),
        is_own: true,
        replies_expanded: true,
        replying_to: Some("日本語🥵".into()),
        quotes_parent: false,
        has_children: false,
        has_draft: true,
    }
}

#[test]
fn t06_every_line_fits_the_width_for_all_attachments() {
    let c = everything();
    for (variant, node) in [
        (Variant::List, Node::Normal),
        (Variant::ThreadMain, Node::Normal),
        (Variant::List, Node::SameAuthor),
    ] {
        for w in [20u16, 40, 68] {
            let lines = lines_of(&c, &busy_opts(variant, node), &ctx(), w);
            assert!(lines.len() > 5, "{variant:?}/{node:?}/{w}: {lines:?}");
            for line in texts(&lines) {
                assert!(
                    line.width() <= usize::from(w),
                    "{variant:?}/{node:?}/{w}: {line:?} is {} wide",
                    line.width()
                );
            }
        }
    }
}

#[test]
fn t06_tombstone_and_deleting_cards_fit_the_width() {
    let t = tombstone();
    for w in [20u16, 40, 68] {
        for input in [
            CardInput::Tombstone(&t),
            CardInput::Deleting {
                short_hash: "a1b2c3d",
            },
        ] {
            let lines = card_lines(input, &opts(), &ctx(), w, &Prefix::default());
            assert!(!lines.is_empty());
            for line in texts(&lines) {
                assert!(line.width() <= usize::from(w), "{w}: {line:?}");
            }
        }
    }
}

#[test]
fn t06_prefix_first_goes_on_the_header_and_rest_on_every_other_line() {
    let c = everything();
    for w in [20u16, 40, 68] {
        let lines = card_lines(
            CardInput::Commit(&c),
            &busy_opts(Variant::List, Node::Normal),
            &ctx(),
            w,
            &prefix(),
        );
        let t = texts(&lines);
        assert!(t.len() > 5);
        assert!(row(&t, 0).starts_with(" ├ ◉ "), "{w}: {:?}", t[0]);
        for line in &t[1..] {
            assert!(line.starts_with(" │ │ "), "{w}: {line:?}");
        }
        for line in &t {
            assert!(line.width() <= usize::from(w), "{w}: {line:?}");
        }
    }
}

// --- 7. selection -----------------------------------------------------------

#[test]
fn t07_selection_replaces_only_the_marker_column_and_keeps_line_count() {
    let c = commit(json!({"quoted_commit": null}));
    for (prefix, ascii, marker) in [
        (Prefix::default(), false, "▌"),
        (prefix(), false, "▌"),
        (Prefix::default(), true, ">"),
    ] {
        let ctx = RenderCtx { ascii, ..ctx() };
        let mut lines = card_lines(CardInput::Commit(&c), &opts(), &ctx, W, &prefix);
        let before = texts(&lines);
        assert!(before.len() > 3);
        apply_selection(&mut lines, &ctx, W);
        let after = texts(&lines);
        assert_eq!(after.len(), before.len());
        for (b, a) in before.iter().zip(&after) {
            assert!(a.starts_with(marker), "{a:?}");
            // Selection pads rows to the full width, so trailing spaces differ.
            assert_eq!(
                a.chars().skip(1).collect::<String>().trim_end(),
                b.chars().skip(1).collect::<String>().trim_end()
            );
        }
    }
}

// --- 8. replying to ---------------------------------------------------------

#[test]
fn t08_replying_to_line_precedes_the_body() {
    let o = CardOptions {
        replying_to: Some("bob".into()),
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ctx(), W);
    assert_eq!(row(&t, 1), &l("↳ replying to @bob"));
    assert_eq!(row(&t, 2), &l("Hello"));
}

// --- 9. link card -----------------------------------------------------------

#[test]
fn t09_pending_link_preview_draws_nothing() {
    let c = commit(json!({"link_preview": link_json(json!({"is_pending": true}))}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(middle(&t), &[l("Hello")]);
}

#[test]
fn t09_link_without_title_and_description_is_a_single_url_line() {
    let c = commit(json!({"link_preview": link_json(json!({
        "title": null, "description": null, "site_name": null,
        "url": "https://example.com/x",
    }))}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(middle(&t), &[l("Hello"), l("https://example.com/x")]);
}

#[test]
fn t09_missing_site_name_falls_back_to_the_url_host() {
    let c = commit(json!({"link_preview": link_json(json!({"site_name": null}))}));
    let t = card_text(&c, &opts(), &ctx(), W);
    let mut want = vec![l("Hello")];
    want.extend(solid_box(
        Some("example.com"),
        &["Example Title", "An example description."],
    ));
    assert_eq!(middle(&t), want);
}

// --- 10. poll ---------------------------------------------------------------

fn poll_footer(closes_at: Option<Duration>, is_closed: bool) -> Option<String> {
    let c = commit(json!({"poll": poll_json(
        closes_at.map(at), is_closed, Some(2), &[("a", 50), ("b", 50)],
    )}));
    let t = card_text(&c, &opts(), &ctx(), W);
    let rows = middle(&t);
    assert_eq!(
        rows[..3],
        [l("Hello"), l("▇▇▇▇▇░░░░░  50% a"), l("▇▇▇▇▇░░░░░  50% b")]
    );
    rows.get(3).cloned()
}

#[test]
fn t10_closed_poll_says_final_results() {
    assert_eq!(
        poll_footer(Some(Duration::days(-1)), true),
        Some(l("Final results"))
    );
}

#[test]
fn t10_open_poll_shows_the_largest_remaining_unit() {
    assert_eq!(
        poll_footer(Some(Duration::days(3) + Duration::hours(2)), false),
        Some(l("3d left"))
    );
    assert_eq!(
        poll_footer(Some(Duration::hours(5) + Duration::minutes(10)), false),
        Some(l("5h left"))
    );
    assert_eq!(
        poll_footer(Some(Duration::minutes(12) + Duration::seconds(30)), false),
        Some(l("12m left"))
    );
    assert_eq!(
        poll_footer(Some(Duration::seconds(30)), false),
        Some(l("<1m left"))
    );
}

#[test]
fn t10_open_poll_without_deadline_has_no_footer() {
    assert_eq!(poll_footer(None, false), None);
}

#[test]
fn t10_bars_use_ten_cells_with_rounded_percentage() {
    let c = commit(json!({"poll": poll_json(
        None, false, Some(10),
        &[("a", 100), ("b", 65), ("c", 64), ("d", 5), ("e", 0)],
    )}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(
        middle(&t),
        &[
            l("Hello"),
            l("▇▇▇▇▇▇▇▇▇▇ 100% a"),
            l("▇▇▇▇▇▇▇░░░  65% b"),
            l("▇▇▇▇▇▇░░░░  64% c"),
            l("▇░░░░░░░░░   5% d"),
            l("░░░░░░░░░░   0% e"),
        ]
    );
}

#[test]
fn t10_poll_without_totals_lists_plain_options() {
    let c = commit(json!({"poll": poll_json(None, false, None, &[("Yes", 0), ("No", 0)])}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(middle(&t), &[l("Hello"), l("○ Yes"), l("○ No")]);
}

// --- control characters -----------------------------------------------------

#[test]
fn t11_escape_nul_and_cr_are_removed_from_guest_body() {
    let body = |s: &str| {
        let t = card_text(&guest_commit(json!({"body": s})), &opts(), &ctx(), W);
        middle(&t).to_vec()
    };
    assert_eq!(body("a\u{1b}[31mb"), vec![l("a[31mb")]);
    assert_eq!(body("a\0b"), vec![l("ab")]);
    assert_eq!(body("a\rb"), vec![l("ab")]);
    assert_eq!(body("a\r\nb"), vec![l("a"), l("b")]);
}

#[test]
fn t11_tab_becomes_spaces_in_body_and_code() {
    let t = card_text(&guest_commit(json!({"body": "a\tb"})), &opts(), &ctx(), W);
    assert_eq!(middle(&t), &[l("a    b")]);

    let c = commit(json!({"body_segments": [block(None, "a\tb")]}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(
        middle(&t),
        &[l("┌────────┐"), l("│ a    b │"), l("└────────┘")]
    );
}

#[test]
fn t11_escape_is_removed_from_the_author_name() {
    let c = commit(json!({"author": author_json("alice", Some("Al\u{1b}[2Jice"), Some(1))}));
    assert_eq!(
        row(&card_text(&c, &opts(), &ctx(), W), 0),
        &hdr("Al[2Jice @alice · 3m")
    );
}

#[test]
fn t11_no_rendered_span_contains_control_characters() {
    let e = "\u{1b}[31m\0\r";
    let c = commit(json!({
        "short_hash": format!("a{e}b"),
        "author": author_json(&format!("al{e}ice"), Some(&format!("Al{e}ice")), Some(1)),
        "body_segments": [
            text(&format!("t{e}x\tt")),
            code(&format!("c{e}d")),
            block(Some(&format!("ru{e}st")), &format!("<span class=\"hl-keyword\">f{e}n</span> m{e}\tx")),
        ],
        "images": [image_json(1, 2, Some(&format!("d{e}esc")))],
        "link_preview": link_json(json!({
            "url": format!("https://ex{e}ample.com/"),
            "title": format!("ti{e}tle"),
            "description": format!("de{e}sc"),
            "site_name": format!("si{e}te"),
        })),
        "poll": poll_json(None, false, Some(1), &[(&format!("la{e}bel"), 100)]),
        "quoted_commit": quoted_json(&format!("qu{e}ote")),
        "reply_preview": {
            "data": [reply_json("bob", &format!("Bo{e}b"), 3, &format!("re{e}ply"), 1)],
            "next_cursor": null,
        },
    }));
    let o = CardOptions {
        replies_expanded: true,
        replying_to: Some(format!("bo{e}b")),
        ..opts()
    };
    let lines = lines_of(&c, &o, &ctx(), W);
    assert!(lines.len() > 10);
    let deleting = card_lines(
        CardInput::Deleting {
            short_hash: &format!("a{e}b"),
        },
        &opts(),
        &ctx(),
        W,
        &Prefix::default(),
    );
    assert!(!deleting.is_empty());
    for line in lines.iter().chain(&deleting) {
        for span in &line.spans {
            assert!(
                !span.content.chars().any(char::is_control),
                "{:?}",
                span.content
            );
        }
    }
}

// --- snapshots --------------------------------------------------------------

#[test]
fn c01_card_basic() {
    let c = commit(json!({"body_segments": [
        text("Hello world, this uses "), code("inline code"), text(" here."),
    ]}));
    snapshot!("card_basic", draw_commit(&c, &opts(), &ctx(), 70, 6));
}

#[test]
fn c02_card_code_block() {
    let html = "<span class=\"hl-keyword\">fn</span> main() {}\n<span class=\"hl-keyword\">let</span> x = 1;";
    let c = commit(json!({"body_segments": [
        text("Try this:"), block(Some("rust"), html),
    ]}));
    snapshot!("card_code_block", draw_commit(&c, &opts(), &ctx(), 70, 8));
}

#[test]
fn c03a_card_quote() {
    let c = commit(json!({
        "body_segments": [text("Look at this:")],
        "quoted_commit": quoted_json("Original thought\nsecond line"),
    }));
    snapshot!("card_quote", draw_commit(&c, &opts(), &ctx(), 70, 8));
}

#[test]
fn c03b_card_quote_unavailable() {
    let c = commit(json!({"body_segments": [text("See this:")], "quoted_commit": null}));
    snapshot!(
        "card_quote_unavailable",
        draw_commit(&c, &opts(), &ctx(), 70, 6)
    );
}

#[test]
fn c03c_card_quote_deleted() {
    let c = commit(json!({
        "body_segments": [text("See this:")],
        "quoted_commit": tombstone_json(),
    }));
    snapshot!(
        "card_quote_deleted",
        draw_commit(&c, &opts(), &ctx(), 70, 6)
    );
}

#[test]
fn c04_card_link_images() {
    let c = commit(json!({
        "body_segments": [text("Check these out:")],
        "images": [
            image_json(1200, 800, None),
            image_json(640, 480, Some("A chart")),
        ],
        "link_preview": link_json(json!({})),
    }));
    snapshot!("card_link_images", draw_commit(&c, &opts(), &ctx(), 70, 9));
}

#[test]
fn c05a_card_poll_open() {
    let c = commit(json!({
        "body_segments": [text("Which one?")],
        "poll": poll_json(
            Some(at(Duration::days(3) + Duration::hours(2))),
            false,
            Some(13),
            &[("Rust", 62), ("Go", 38)],
        ),
    }));
    snapshot!("card_poll_open", draw_commit(&c, &opts(), &ctx(), 70, 8));
}

#[test]
fn c05b_card_poll_closed() {
    let c = commit(json!({
        "body_segments": [text("Which one?")],
        "poll": poll_json(
            Some(at(Duration::days(-1))),
            true,
            Some(13),
            &[("Rust", 62), ("Go", 38)],
        ),
    }));
    snapshot!("card_poll_closed", draw_commit(&c, &opts(), &ctx(), 70, 8));
}

#[test]
fn c06_card_replies_expanded() {
    let c = commit(json!({
        "body_segments": [text("Parent body.")],
        "replies_count": 5,
        "reply_preview": {"data": [bob(), tombstone_json(), carol()], "next_cursor": null},
    }));
    snapshot!(
        "card_replies_expanded",
        draw_commit(&c, &expanded(), &ctx(), 70, 14)
    );
}

#[test]
fn c07_card_tombstone() {
    let t = tombstone();
    let lines = card_lines(
        CardInput::Tombstone(&t),
        &opts(),
        &ctx(),
        70,
        &Prefix::default(),
    );
    snapshot!("card_tombstone", terminal_of(lines, 70, 2));
}

#[test]
fn c08_card_ascii() {
    let c = commit(json!({
        "author": author_json("alice", Some("Alice"), Some(1)),
        "body_segments": [text("Hello ASCII world")],
        "poll": poll_json(
            Some(at(Duration::days(3) + Duration::hours(2))),
            false,
            None,
            &[("Yes", 0), ("No", 0)],
        ),
        "quoted_commit": quoted_json("Original thought"),
        "reply_preview": {"data": [bob(), tombstone_json()], "next_cursor": null},
    }));
    let ascii = RenderCtx {
        ascii: true,
        ..ctx()
    };
    let terminal = draw_commit(&c, &expanded(), &ascii, 70, 16);
    let buffer = terminal.backend().buffer();
    assert!(
        buffer.content().iter().all(|cell| cell.symbol().is_ascii()),
        "ASCII mode must not emit non-ASCII symbols"
    );
    snapshot!("card_ascii", terminal);
}

#[test]
fn c09_card_same_author() {
    let o = CardOptions {
        node: Node::SameAuthor,
        thread_pos: Some((2, 2)),
        ..opts()
    };
    let c = commit(json!({"body_segments": [text("Same author follow-up.")]}));
    snapshot!("card_same_author", draw_commit(&c, &o, &ctx(), 70, 5));
}

#[test]
fn c10_card_folded() {
    let c = commit(json!({
        "body_segments": [text("Not shown in the list because the preview is used instead.")],
        "preview_segments": [text("This is a long commit message about folding. It keeps going well past the first line so that the preview wraps onto a second line…")],
    }));
    snapshot!("card_folded", draw_commit(&c, &opts(), &ctx(), 70, 12));
}

#[test]
fn c11_card_thread_main() {
    let o = CardOptions {
        thread_pos: Some((1, 2)),
        ..thread_main()
    };
    let c = commit(json!({
        "created_at": "2026-10-18T14:02:00Z",
        "body_segments": [
            text("Full body shown in the thread view."),
            block(Some("rust"), "fn main() {}"),
            text("Tail text after the block."),
        ],
        "preview_segments": [text("Full body…")],
    }));
    snapshot!("card_thread_main", draw_commit(&c, &o, &ctx(), 70, 10));
}

#[test]
fn c12_card_narrow_40() {
    let narrow = RenderCtx {
        pasteis_mode: false,
        ..ctx()
    };
    let html = "<span class=\"hl-keyword\">let</span> very_long_identifier_name = compute_something_large(1, 2, 3);\nok();";
    let c = commit(json!({"body_segments": [
        text("The quick brown fox jumps over the lazy dog and "),
        code("keeps"),
        text(" running through the forest."),
        block(Some("rust"), html),
    ]}));
    snapshot!("card_narrow_40", draw_commit(&c, &opts(), &narrow, 40, 14));
}

#[test]
fn c13_card_replying_to() {
    let o = CardOptions {
        replying_to: Some("bob".into()),
        ..opts()
    };
    let c = commit(json!({"body_segments": [text("Agreed, shipping it.")]}));
    snapshot!("card_replying_to", draw_commit(&c, &o, &ctx(), 70, 5));
}

#[test]
fn c14_card_deleting() {
    let lines = card_lines(
        CardInput::Deleting {
            short_hash: "a1b2c3d",
        },
        &opts(),
        &ctx(),
        70,
        &Prefix::default(),
    );
    snapshot!("card_deleting", terminal_of(lines, 70, 3));
}

// --- styles -----------------------------------------------------------------

fn lane1() -> Color {
    Color::Rgb(0x51, 0xcf, 0x66)
}

#[test]
fn style_rail_and_node_use_the_author_lane_color() {
    let lines = lines_of(&commit(json!({})), &opts(), &ctx(), W);
    let buffer = buffer_of(lines, W, 4);
    assert_eq!(buffer[(1, 0)].symbol(), "◉");
    assert_eq!(buffer[(1, 0)].fg, lane1());
    assert_eq!(buffer[(1, 1)].symbol(), "│");
    assert_eq!(buffer[(1, 1)].fg, lane1());
}

#[test]
fn style_rail_falls_back_to_the_automatic_lane_without_truecolor() {
    let auto = commit(json!({"author": author_json("alice", Some("Alice"), None)}));
    let want = rgb(Lane::Index(auto_lane("user-alice")));
    let buffer = buffer_of(lines_of(&auto, &opts(), &ctx(), W), W, 4);
    assert_eq!(buffer[(1, 1)].symbol(), "│");
    assert_eq!(buffer[(1, 1)].fg, Color::Rgb(want.0, want.1, want.2));

    let indexed = RenderCtx {
        truecolor: false,
        ..ctx()
    };
    let buffer = buffer_of(lines_of(&commit(json!({})), &opts(), &indexed, W), W, 4);
    assert_eq!(buffer[(1, 1)].symbol(), "│");
    assert_eq!(
        buffer[(1, 1)].fg,
        Color::Indexed(to_ansi256(Rgb(0x51, 0xcf, 0x66)))
    );
}

#[test]
fn style_starred_icon_uses_the_primary_color() {
    let lines = lines_of(&commit(json!({"starred": true})), &opts(), &ctx(), W);
    let buffer = buffer_of(lines, W, 4);
    let (x, y) = find_symbol(&buffer, "★").unwrap_or_default();
    assert_eq!(buffer[(x, y)].symbol(), "★", "symbol missing");
    assert_eq!(buffer[(x, y)].fg, theme::PRIMARY);
    let (x, y) = find_symbol(&buffer, "◓").unwrap_or_default();
    assert_eq!(buffer[(x, y)].symbol(), "◓", "symbol missing");
    assert_ne!(buffer[(x, y)].fg, theme::PRIMARY);
}

#[test]
fn style_inline_code_has_the_code_background() {
    let c = commit(json!({"body_segments": [text("a "), code("xyz"), text(" b")]}));
    let buffer = buffer_of(lines_of(&c, &opts(), &ctx(), W), W, 4);
    assert_eq!(buffer[(5, 1)].symbol(), "x");
    for x in 5..8 {
        assert_eq!(buffer[(x, 1)].bg, theme::code_bg(true));
    }
    assert_ne!(buffer[(3, 1)].bg, theme::code_bg(true));
    assert_ne!(buffer[(9, 1)].bg, theme::code_bg(true));
}

#[test]
fn style_selection_fills_the_whole_row_with_the_cursor_background() {
    let c = commit(json!({}));
    let mut lines = lines_of(&c, &opts(), &ctx(), W);
    let rows = lines.len() as u16;
    assert!(rows >= 3, "expected a card, got {lines:?}");
    apply_selection(&mut lines, &ctx(), W);
    let buffer = buffer_of(lines, W, rows);
    for y in 0..rows {
        assert_eq!(buffer[(0, y)].symbol(), "▌");
        for x in [0, 3, W - 1] {
            assert_eq!(buffer[(x, y)].bg, theme::cursor_bg(true), "({x}, {y})");
        }
    }
}

#[test]
fn style_tombstone_is_dimmed() {
    let t = tombstone();
    let lines = card_lines(
        CardInput::Tombstone(&t),
        &opts(),
        &ctx(),
        W,
        &Prefix::default(),
    );
    let buffer = buffer_of(lines, W, 2);
    assert_eq!(buffer[(3, 0)].symbol(), "[");
    assert!(buffer[(1, 0)].modifier.contains(Modifier::DIM));
    assert!(buffer[(3, 0)].modifier.contains(theme::DIM));
}

#[test]
fn style_code_block_keyword_uses_the_theme_keyword_color() {
    let html = "<span class=\"hl-keyword\">fn</span> main() {}";
    let c = commit(json!({"body_segments": [block(Some("rust"), html)]}));
    let buffer = buffer_of(lines_of(&c, &opts(), &ctx(), W), W, 6);
    assert_eq!(buffer[(5, 2)].symbol(), "f");
    assert_eq!(buffer[(5, 2)].fg, theme::hl_color(HlClass::Keyword, true));
    assert_eq!(buffer[(8, 2)].symbol(), "m");
    assert_eq!(buffer[(8, 2)].fg, theme::hl_color(HlClass::Plain, true));
    assert_ne!(
        theme::hl_color(HlClass::Keyword, true),
        theme::hl_color(HlClass::Plain, true)
    );
}

#[test]
fn style_reply_preview_colors_branch_by_parent_lane_and_dot_by_replier_lane() {
    let c = with_replies(vec![bob()], 1, None);
    let buffer = buffer_of(lines_of(&c, &expanded(), &ctx(), W), W, 6);
    assert_eq!(buffer[(3, 3)].symbol(), "└");
    assert_eq!(buffer[(3, 3)].fg, lane1());
    assert_eq!(buffer[(5, 3)].symbol(), "●");
    assert_eq!(buffer[(5, 3)].fg, Color::Rgb(0x3b, 0xc9, 0xdb));
}

// --- review follow-ups ------------------------------------------------------

fn la(s: &str) -> String {
    format!(" | {s}")
}

fn ascii_ctx() -> RenderCtx {
    RenderCtx {
        ascii: true,
        ..ctx()
    }
}

#[test]
fn t09_link_card_puts_site_name_in_the_top_border() {
    let c = commit(json!({"link_preview": link_json(json!({}))}));
    let t = card_text(&c, &opts(), &ctx(), W);
    let mut want = vec![l("Hello")];
    want.extend(solid_box(
        Some("Example Site"),
        &["Example Title", "An example description."],
    ));
    assert_eq!(middle(&t), want);
}

#[test]
fn t09_link_card_title_and_description_are_cut_at_2_lines_with_ellipsis() {
    let words = |w: &str| vec![w.repeat(10); 20].join(" ");
    let c = commit(json!({"link_preview": link_json(json!({
        "title": words("a"),
        "description": words("b"),
    }))}));
    let t = card_text(&c, &opts(), &ctx(), W);
    let five = |w: &str| vec![w.repeat(10); 5].join(" ");
    let (a, b) = (five("a"), five("b"));
    let (a_last, b_last) = (format!("{a}…"), format!("{b}…"));
    let mut want = vec![l("Hello")];
    want.extend(solid_box(Some("Example Site"), &[&a, &a_last, &b, &b_last]));
    assert_eq!(middle(&t), want);
}

#[test]
fn t12_double_width_char_that_would_straddle_the_edge_moves_to_the_next_line() {
    // width 10 leaves 7 columns for text: "aaaaaa" + a 2-column char would need 8.
    let c = guest_commit(json!({"body": "aaaaaa日本"}));
    let t = card_text(&c, &opts(), &ctx(), 10);
    assert_eq!(row(&t, 1), &l("aaaaaa"));
    assert_eq!(row(&t, 2), &l("日本"));
}

#[test]
fn t13_thread_main_date_line_uses_the_local_offset() {
    let jst = RenderCtx {
        local_offset: UtcOffset::from_hms(9, 0, 0).unwrap(),
        ..ctx()
    };
    let c = commit(json!({"created_at": "2026-10-18T20:02:00Z"}));
    let t = card_text(&c, &thread_main(), &jst, W);
    assert_eq!(row(&t, 2), &l("Oct 19, 2026 05:02 · commit a1b2c3d"));
}

#[test]
fn t14_ascii_poll_with_votes_uses_hash_and_dot_bars() {
    let c = commit(json!({"poll": poll_json(None, false, Some(13), &[("Rust", 62), ("Go", 38)])}));
    let t = card_text(&c, &opts(), &ascii_ctx(), W);
    assert_eq!(
        middle(&t),
        &[
            la("Hello"),
            la("######....  62% Rust"),
            la("####......  38% Go")
        ]
    );
}

#[test]
fn t14_ascii_unavailable_quote_uses_plus_dot_colon_box() {
    let c = commit(json!({"quoted_commit": null}));
    let t = card_text(&c, &opts(), &ascii_ctx(), W);
    let mut want = vec![la("Hello")];
    want.extend(
        framed_with(
            ASCII_DOTTED,
            67,
            None,
            &["[unavailable] This commit isn't available."],
        )
        .iter()
        .map(|s| la(s)),
    );
    assert_eq!(middle(&t), want);
}

#[test]
fn t14_ascii_replying_to_uses_angle_bracket() {
    let o = CardOptions {
        replying_to: Some("bob".into()),
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ascii_ctx(), W);
    assert_eq!(row(&t, 1), &la("> replying to @bob"));
}

#[test]
fn t14_ascii_starred_and_saved_use_star_and_checked_box() {
    let c = commit(json!({"starred": true, "is_saved": true}));
    let t = card_text(&c, &opts(), &ascii_ctx(), W);
    assert_eq!(
        last(&t),
        &pad_between(
            " | ",
            "* 12  p 3  \" Quote  < Reply  v 2 replies",
            "[x] !",
            W
        )
    );
}

#[test]
fn t14_ascii_same_author_node_is_a_star() {
    let o = CardOptions {
        node: Node::SameAuthor,
        thread_pos: Some((2, 2)),
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ascii_ctx(), W);
    assert_eq!(
        row(&t, 0),
        &pad_between(" * ", "Alice  [2/2]", "a1b2c3d", W)
    );
}

#[test]
fn t14_ascii_tombstone_node_is_x() {
    let t = tombstone();
    let lines = card_lines(
        CardInput::Tombstone(&t),
        &opts(),
        &ascii_ctx(),
        W,
        &Prefix::default(),
    );
    assert_eq!(texts(&lines), vec![" x [deleted] This commit was deleted."]);
}

#[test]
fn t14_ascii_deleting_uses_arrow_and_plus_dash_box() {
    let lines = card_lines(
        CardInput::Deleting {
            short_hash: "a1b2c3d",
        },
        &opts(),
        &ascii_ctx(),
        W,
        &Prefix::default(),
    );
    let want: Vec<String> = framed_with(ASCII_SOLID, 27, None, &["rm a1b2c3d -> /dev/null"])
        .iter()
        .map(|s| format!(" {s}"))
        .collect();
    assert_eq!(texts(&lines), want);
}

#[test]
fn t14_ascii_truncation_uses_three_dots() {
    let c = commit(json!({"author": author_json("alice", Some("Alexander the Great"), Some(1))}));
    let t = card_text(&c, &opts(), &ascii_ctx(), 30);
    assert_eq!(row(&t, 0), " o Alex... @alice - 3m a1b2c3d");
}

#[test]
fn style_truecolor_off_makes_theme_colors_indexed() {
    let indexed = RenderCtx {
        truecolor: false,
        ..ctx()
    };
    let html = "<span class=\"hl-keyword\">fn</span> main() {}";
    let c = commit(json!({"body_segments": [block(Some("rust"), html)]}));
    let buffer = buffer_of(lines_of(&c, &opts(), &indexed, W), W, 6);
    assert_eq!(buffer[(5, 2)].symbol(), "f");
    assert_eq!(buffer[(5, 2)].fg, theme::hl_color(HlClass::Keyword, false));
    assert!(matches!(buffer[(5, 2)].fg, Color::Indexed(_)));

    let c = commit(json!({"body_segments": [text("a "), code("xyz")]}));
    let buffer = buffer_of(lines_of(&c, &opts(), &indexed, W), W, 4);
    assert_eq!(buffer[(5, 1)].symbol(), "x");
    assert_eq!(buffer[(5, 1)].bg, theme::code_bg(false));
    assert!(matches!(buffer[(5, 1)].bg, Color::Indexed(_)));
}

// --- implementation review follow-ups ---------------------------------------

#[test]
fn t06_emoji_clusters_are_measured_and_wrapped_whole() {
    let family = "👨\u{200d}👩\u{200d}👧";
    let body = format!("{} {}", "❤\u{fe0f}".repeat(20), family.repeat(10));
    let c = guest_commit(json!({"body": body}));
    for w in [20u16, 40] {
        let t = card_text(&c, &opts(), &ctx(), w);
        for line in &t {
            assert!(line.width() <= usize::from(w), "{w}: {line:?}");
            assert!(!line.contains("\u{fe0f} ") || line.contains('❤'));
        }
        let all = t.join("\n");
        assert_eq!(all.matches("❤\u{fe0f}").count(), 20, "{w}: {t:#?}");
        assert_eq!(all.matches('❤').count(), 20, "{w}: {t:#?}");
        assert_eq!(all.matches(family).count(), 10, "{w}: {t:#?}");
        assert_eq!(all.matches('\u{200d}').count(), 20, "{w}: {t:#?}");
    }
}

#[test]
fn t15_ascii_mode_cuts_long_box_titles_with_three_dots() {
    let long = "x".repeat(100);
    let c = commit(json!({
        "body_segments": [block(Some(&long), "fn main() {}")],
        "link_preview": link_json(json!({"site_name": long})),
        "quoted_commit": quoted_json("q"),
    }));
    let t = card_text(&c, &opts(), &ascii_ctx(), W);
    assert!(t.iter().any(|s| s.contains("...")), "{t:#?}");
    for line in &t {
        assert!(line.is_ascii(), "{line:?}");
        assert!(line.width() <= usize::from(W), "{line:?}");
    }
}

#[test]
fn t15_newlines_in_single_line_fields_become_spaces() {
    let c = commit(json!({
        "short_hash": "a1\nb2c3d",
        "author": author_json("alice", Some("Al\nice"), Some(1)),
        "poll": poll_json(None, false, None, &[("Y\nes", 0)]),
        "link_preview": link_json(json!({"site_name": "si\nte"})),
        "images": [image_json(1, 2, Some("de\nsc"))],
    }));
    let o = CardOptions {
        replying_to: Some("b\nob".into()),
        ..opts()
    };
    let t = card_text(&c, &o, &ctx(), W);
    for line in &t {
        assert!(!line.contains('\n'), "{line:?}");
    }
    assert!(row(&t, 0).contains("Al ice @alice"), "{t:#?}");
    assert!(row(&t, 0).ends_with("a1 b2c3d"), "{t:#?}");
    assert_eq!(row(&t, 1), &l("↳ replying to @b ob"));
    assert!(t.contains(&l("○ Y es")), "{t:#?}");
}

#[test]
fn t15_indent_wider_than_the_line_is_dropped_instead_of_the_word() {
    let c = guest_commit(json!({"body": format!("{}word", " ".repeat(40))}));
    let t = card_text(&c, &opts(), &ctx(), 30);
    assert_eq!(row(&t, 1), &l("word"));
}

#[test]
fn t15_trailing_newlines_do_not_count_as_lines_when_folding() {
    let body = format!("{}\n\n\n", ["x"; 8].join("\n"));
    let t = card_text(&guest_commit(json!({"body": body})), &opts(), &ctx(), W);
    assert!(t.iter().all(|s| !s.contains("Show more")), "{t:#?}");
}

#[test]
fn t15_prefix_halves_of_different_width_keep_the_rail_aligned() {
    let narrow_rest = Prefix {
        first: vec![Span::raw("├ ")],
        rest: vec![Span::raw("│")],
    };
    let c = commit(json!({}));
    let t = texts(&card_lines(
        CardInput::Commit(&c),
        &opts(),
        &ctx(),
        W,
        &narrow_rest,
    ));
    assert!(row(&t, 0).starts_with(" ├ ◉ "), "{t:#?}");
    for line in &t[1..] {
        assert!(line.starts_with(" │ │ "), "{line:?}");
    }
}

#[test]
fn t15_empty_body_draws_no_blank_rail_row() {
    let t = card_text(&guest_commit(json!({"body": ""})), &opts(), &ctx(), W);
    assert_eq!(t.len(), 2, "{t:#?}");
}

#[test]
fn t15_open_poll_past_its_deadline_says_final_results() {
    assert_eq!(
        poll_footer(Some(Duration::minutes(-5)), false),
        Some(l("Final results"))
    );
}

#[test]
fn t15_empty_link_title_and_description_count_as_missing() {
    let c = commit(json!({"link_preview": link_json(json!({
        "title": "", "description": "", "url": "https://example.com/x",
    }))}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(middle(&t), &[l("Hello"), l("https://example.com/x")]);
}

#[test]
fn t15_bidi_overrides_are_removed() {
    let c = guest_commit(json!({"body": "a\u{202e}b\u{200f}c\u{2066}d"}));
    let t = card_text(&c, &opts(), &ctx(), W);
    assert_eq!(middle(&t), &[l("abcd")]);
}

#[test]
fn t15_tiny_width_never_overflows_even_with_the_ellipsis() {
    let c = everything();
    for w in [6u16, 8, 10] {
        for line in card_text(&c, &opts(), &ctx(), w) {
            assert!(line.width() <= usize::from(w), "{w}: {line:?}");
        }
    }
}

// --- 13. empty bodies and the quote marker ----------------------------------

#[test]
fn t13_reply_preview_with_an_empty_body_draws_no_blank_row() {
    let empty = reply_json("bob", "Bob", 3, "", 1);
    let c = with_replies(vec![empty, carol()], 2, None);
    let t = card_text(&c, &expanded(), &ctx(), W);
    assert_eq!(
        rows_from(&t, 3),
        [
            l("├─● Bob @bob · 1m"),
            l("└─● Carol @carol · 2m"),
            l("      Third reply"),
        ]
    );
}

#[test]
fn t13_quote_with_an_empty_body_draws_a_box_without_body_rows() {
    let t = card_text(
        &commit(json!({"quoted_commit": quoted_json("")})),
        &opts(),
        &ctx(),
        W,
    );
    let mut want = vec![l("Hello")];
    want.extend(solid_box(Some("❝ Bob @bob · 2h"), &[]));
    assert_eq!(middle(&t), want);
}

#[test]
fn t13_quote_marker_precedes_the_body() {
    let o = CardOptions {
        quotes_parent: true,
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ctx(), W);
    assert_eq!(row(&t, 1), &l("❝ quoted this commit"));
    assert_eq!(row(&t, 2), &l("Hello"));
}

#[test]
fn t13_quote_marker_takes_the_place_of_replying_to() {
    let o = CardOptions {
        quotes_parent: true,
        replying_to: Some("bob".into()),
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ctx(), W);
    assert_eq!(row(&t, 1), &l("❝ quoted this commit"));
    assert!(t.iter().all(|s| !s.contains("replying to")), "{t:#?}");
}

#[test]
fn t13_ascii_quote_marker_uses_a_straight_quote() {
    let o = CardOptions {
        quotes_parent: true,
        ..opts()
    };
    let t = card_text(&commit(json!({})), &o, &ascii_ctx(), W);
    assert_eq!(row(&t, 1), &la("\" quoted this commit"));
}

// --- 16. reply variant ------------------------------------------------------

fn reply_opts() -> CardOptions {
    CardOptions {
        variant: Variant::Reply,
        ..opts()
    }
}

fn branch(first: &str, rest: &str) -> Prefix {
    Prefix {
        first: vec![Span::raw(first.to_string())],
        rest: vec![Span::raw(rest.to_string())],
    }
}

fn reply_text(c: &Commit, o: &CardOptions, p: &Prefix) -> Vec<String> {
    texts(&card_lines(CardInput::Commit(c), o, &ctx(), W, p))
}

#[test]
fn t16_reply_has_a_dot_node_no_hash_an_indented_body_and_minimal_actions() {
    let t = reply_text(&commit(json!({})), &reply_opts(), &branch("├─", "│ "));
    assert_eq!(
        t,
        vec![
            " ├─● Alice @alice · 3m".to_string(),
            " │     Hello".to_string(),
            " │     ☆ 12  ↩ Reply".to_string(),
        ]
    );
}

#[test]
fn t16_last_reply_leaves_the_guide_column_blank() {
    let t = reply_text(&commit(json!({})), &reply_opts(), &branch("└─", "  "));
    assert_eq!(
        t,
        vec![
            " └─● Alice @alice · 3m".to_string(),
            "       Hello".to_string(),
            "       ☆ 12  ↩ Reply".to_string(),
        ]
    );
}

#[test]
fn t16_reply_puts_replying_to_and_the_quote_marker_at_the_body_column() {
    let o = CardOptions {
        replying_to: Some("bob".into()),
        ..reply_opts()
    };
    let t = reply_text(&commit(json!({})), &o, &branch("└─", "  "));
    assert_eq!(row(&t, 1), "       ↳ replying to @bob");
    let q = CardOptions {
        quotes_parent: true,
        ..reply_opts()
    };
    let t = reply_text(&commit(json!({})), &q, &branch("└─", "  "));
    assert_eq!(row(&t, 1), "       ❝ quoted this commit");
}

#[test]
fn t16_reply_never_shows_a_replies_count() {
    let t = reply_text(
        &commit(json!({"replies_count": 5})),
        &reply_opts(),
        &branch("├─", "│ "),
    );
    assert!(t.iter().all(|s| !s.contains("replies")), "{t:#?}");
}

#[test]
fn t16_every_reply_line_fits_the_width() {
    let c = commit(
        json!({"body_segments": [text("a long reply body that has to wrap onto several lines at narrow widths")]}),
    );
    for w in [30u16, 40, 70] {
        let t = texts(&card_lines(
            CardInput::Commit(&c),
            &reply_opts(),
            &ctx(),
            w,
            &branch("│ ├─", "│ │ "),
        ));
        assert!(t.iter().all(|s| s.width() <= usize::from(w)), "{w}: {t:#?}");
    }
}

#[test]
fn t16_reply_with_children_draws_the_rail_in_the_node_column() {
    let o = CardOptions {
        has_children: true,
        ..reply_opts()
    };
    let t = reply_text(&commit(json!({})), &o, &branch("├─", "│ "));
    assert_eq!(
        t,
        vec![
            " ├─● Alice @alice · 3m".to_string(),
            " │ │   Hello".to_string(),
            " │ │   ☆ 12  ↩ Reply".to_string(),
        ]
    );
    let t = reply_text(&commit(json!({})), &o, &branch("└─", "  "));
    assert_eq!(row(&t, 1), "   │   Hello");
    assert_eq!(row(&t, 2), "   │   ☆ 12  ↩ Reply");
}

#[test]
fn t16_reply_shows_the_draft_label_and_the_filled_star() {
    let o = CardOptions {
        has_draft: true,
        ..reply_opts()
    };
    let t = reply_text(&commit(json!({"starred": true})), &o, &branch("├─", "│ "));
    assert_eq!(row(&t, 2), " │     ★ 12  ↩ stash@{0}");
}
