use serde::Deserialize;
use serde_json::{Value, json};
use time::macros::datetime;

use super::*;

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> T {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("deserialize failed: {e}"))
}

fn parse_value<T: for<'de> Deserialize<'de>>(value: Value) -> T {
    serde_json::from_value(value).unwrap_or_else(|e| panic!("deserialize failed: {e}"))
}

fn user_json(extra: Value) -> Value {
    let mut v = json!({
        "id": "u1",
        "username": "alice",
        "name": "Alice",
        "avatar_url": "https://example.com/a.png",
        "created_at": "2026-10-15T13:02:44Z",
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

fn commit_json(extra: Value) -> Value {
    let mut v = json!({
        "id": "01a15000-0000-7000-8000-0000000b9b9b",
        "short_hash": "b9b9b9b",
        "body": "hello",
        "created_at": "2026-10-15T13:02:44Z",
        "author": user_json(json!({})),
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

fn tombstone_json() -> Value {
    json!({
        "id": "01a15002-0000-7000-8000-00000000d0d0",
        "parent_id": "01a15000-0000-7000-8000-0000000b9b9b",
        "deleted_at": "2026-10-16T09:00:00Z",
    })
}

fn lane_of(lane: Value) -> Option<Lane> {
    let user: User = parse_value(user_json(json!({ "lane": lane })));
    user.lane
}

// 1. 実データ user.json

#[test]
fn t01_real_user_has_username_counts_and_created_at_and_no_lane() {
    let user = parse::<Data<User>>(include_str!("../../tests/fixtures/user.json")).data;
    assert_eq!(user.username, "nunomaduro");
    assert_eq!(user.commits_count, Some(187));
    assert_eq!(user.followers_count, Some(3679));
    assert_eq!(user.following_count, Some(87));
    assert_eq!(user.created_at, datetime!(2026-10-15 13:02:44 UTC));
    assert_eq!(user.lane, None);
}

// 2. 実データ commits.json

#[test]
fn t02_real_commits_page_has_20_items_and_next_cursor() {
    let page: Page<Commit> = parse(include_str!("../../tests/fixtures/commits.json"));
    assert_eq!(page.data.len(), 20);
    let cursor = page.meta.next_cursor.expect("next_cursor");
    assert!(cursor.starts_with("eyJjcmVhdGVkX2F0"));
}

#[test]
fn t02_real_commits_are_public_variant_without_signed_fields() {
    let page: Page<Commit> = parse(include_str!("../../tests/fixtures/commits.json"));
    for commit in &page.data {
        assert_eq!(commit.body_segments, None);
        assert_eq!(commit.quoted_commit, Quoted::NotLoaded);
    }
}

#[test]
fn t02_real_commit_body_keeps_emoji_as_is() {
    let page: Page<Commit> = parse(include_str!("../../tests/fixtures/commits.json"));
    assert_eq!(page.data[19].body, "and 4,000 users now \u{1F975}");
}

// 3. 実データ commit.json

#[test]
fn t03_real_commit_has_link_preview_and_image_size() {
    let commit = parse::<Data<Commit>>(include_str!("../../tests/fixtures/commit.json")).data;
    let preview = commit.link_preview.expect("link_preview");
    assert_eq!(preview.site_name.as_deref(), Some("/dev/null"));
    assert_eq!(preview.title.as_deref(), Some("Brand - /dev/null"));
    assert_eq!(commit.images.len(), 1);
    assert_eq!(
        (commit.images[0].width, commit.images[0].height),
        (1600, 900)
    );
}

// 4. 実データ followers.json

#[test]
fn t04_real_followers_page_has_no_counts_and_accepts_null_bio() {
    let page: Page<User> = parse(include_str!("../../tests/fixtures/followers.json"));
    assert_eq!(page.data.len(), 30);
    assert!(page.meta.next_cursor.is_some());
    assert!(page.data.iter().all(|u| u.commits_count.is_none()));
    assert!(page.data.iter().any(|u| u.bio.is_none()));
    assert_eq!(page.data[0].username, "0xgdr");
}

// 5. commit_signed.json

fn signed() -> Commit {
    parse::<Data<Commit>>(include_str!("../../tests/fixtures/commit_signed.json")).data
}

#[test]
fn t05_signed_body_segments_cover_text_code_and_both_block_kinds() {
    let segments = signed().body_segments.expect("body_segments");
    assert_eq!(
        segments,
        vec![
            Segment::Text {
                text: "hi @ab see".into()
            },
            Segment::Block {
                language: Some("php".into()),
                html: "<span class=\"hl-keyword\">echo</span> 1;".into()
            },
            Segment::Text {
                text: "use ".into()
            },
            Segment::Code { code: "x".into() },
            Segment::Block {
                language: None,
                html: "plain".into()
            },
        ]
    );
}

#[test]
fn t05_signed_preview_segments_and_mentions() {
    let commit = signed();
    assert_eq!(
        commit.preview_segments,
        Some(vec![Segment::Text {
            text: "hi @ab see…".into()
        }])
    );
    assert_eq!(
        commit.mentions,
        Some(vec![Mention {
            username: "ab".into(),
            offset: 3,
            length: 3
        }])
    );
}

#[test]
fn t05_signed_status_flags_counts_and_author_lane() {
    let commit = signed();
    assert_eq!(commit.status, Some(Status::Pending));
    assert_eq!(commit.starred, Some(true));
    assert_eq!(commit.is_saved, Some(true));
    assert_eq!(commit.pasteis_count, Some(3));
    assert_eq!(commit.author.lane, Some(Lane::Index(3)));
}

#[test]
fn t05_signed_poll_without_votes_visible() {
    let poll = signed().poll.expect("poll");
    assert_eq!(poll.closes_at, Some(datetime!(2026-10-25 00:00:00 UTC)));
    assert!(!poll.is_closed);
    assert_eq!(poll.total_votes, None);
    assert_eq!(poll.options.len(), 2);
    assert_eq!(poll.options[0].label, "yes");
    assert_eq!(poll.options[0].percentage, None);
}

#[test]
fn t05_signed_reply_preview_mixes_commit_and_tombstone_with_cursor() {
    let preview = signed().reply_preview.expect("reply_preview");
    assert_eq!(preview.next_cursor.as_deref(), Some("cursor-rp"));
    assert_eq!(preview.data.len(), 2);
    assert!(matches!(&preview.data[0], ThreadItem::Commit(c) if c.body == "first reply"));
    assert!(matches!(&preview.data[1], ThreadItem::Tombstone(_)));
}

// 6. quoted_commit の 3 状態

#[test]
fn t06_quoted_commit_key_absent_is_not_loaded() {
    let commit: Commit = parse_value(commit_json(json!({})));
    assert_eq!(commit.quoted_commit, Quoted::NotLoaded);
}

#[test]
fn t06_quoted_commit_null_is_unavailable() {
    let commit: Commit = parse_value(commit_json(json!({ "quoted_commit": null })));
    assert_eq!(commit.quoted_commit, Quoted::Unavailable);
}

#[test]
fn t06_quoted_commit_object_with_author_is_item_commit() {
    let quoted = commit_json(json!({ "id": "quoted-1", "body": "quoted body" }));
    let commit: Commit = parse_value(commit_json(json!({ "quoted_commit": quoted })));
    match commit.quoted_commit {
        Quoted::Item(item) => {
            assert!(matches!(*item, ThreadItem::Commit(c) if c.body == "quoted body"))
        }
        other => panic!("expected Item(Commit), got {other:?}"),
    }
}

#[test]
fn t06_quoted_commit_tombstone_is_item_tombstone() {
    let commit: Commit = parse_value(commit_json(json!({ "quoted_commit": tombstone_json() })));
    match commit.quoted_commit {
        Quoted::Item(item) => {
            assert!(matches!(*item, ThreadItem::Tombstone(t) if t.id.ends_with("d0d0")))
        }
        other => panic!("expected Item(Tombstone), got {other:?}"),
    }
}

// 7. replies_with_tombstone.json

#[test]
fn t07_replies_page_distinguishes_tombstone_from_commit() {
    let page: Page<ThreadItem> = parse(include_str!(
        "../../tests/fixtures/replies_with_tombstone.json"
    ));
    assert_eq!(page.data.len(), 2);
    assert!(matches!(&page.data[0], ThreadItem::Tombstone(t)
        if t.deleted_at == datetime!(2026-10-16 09:00:00 UTC)));
    assert!(matches!(&page.data[1], ThreadItem::Commit(c) if c.body == "alive reply"));
    assert_eq!(page.meta.next_cursor, None);
}

#[test]
fn t07_commit_with_author_but_broken_created_at_is_an_error() {
    let good = json!([commit_json(json!({}))]);
    let items: Vec<ThreadItem> = parse_value(good);
    assert!(matches!(&items[0], ThreadItem::Commit(_)));
    let items = json!([commit_json(json!({ "created_at": 12345 }))]);
    assert!(serde_json::from_value::<Vec<ThreadItem>>(items).is_err());
}

#[test]
fn t07_object_without_author_with_deleted_at_string_is_a_tombstone() {
    let items: Vec<ThreadItem> = parse_value(json!([tombstone_json()]));
    assert!(matches!(&items[0], ThreadItem::Tombstone(_)));
}

#[test]
fn t07_object_without_author_and_without_deleted_at_is_an_error() {
    let items = json!([{ "id": "x", "parent_id": null, "deleted_at": null }]);
    assert!(serde_json::from_value::<Vec<ThreadItem>>(items).is_err());
}

// 8. Lane

#[test]
fn t08_lane_integer_in_range_is_index() {
    assert_eq!(lane_of(json!(3)), Some(Lane::Index(3)));
}

#[test]
fn t08_lane_hex_string_is_custom_rgb() {
    assert_eq!(
        lane_of(json!("#ff8800")),
        Some(Lane::Custom(Rgb(0xff, 0x88, 0x00)))
    );
}

#[test]
fn t08_lane_out_of_range_integer_is_none() {
    assert_eq!(lane_of(json!(7)), None);
}

#[test]
fn t08_lane_invalid_string_is_none_without_failing_the_user() {
    assert_eq!(lane_of(json!("red")), None);
}

#[test]
fn t08_lane_key_absent_is_none() {
    let user: User = parse_value(user_json(json!({})));
    assert_eq!(user.lane, None);
}

#[test]
fn t08_lane_boundary_values_are_none() {
    for v in [
        json!(null),
        json!(-1),
        json!(3.5),
        json!("#FF8800"),
        json!("#ff88"),
    ] {
        assert_eq!(lane_of(v.clone()), None, "{v}");
    }
}

// 9. 封筒

#[test]
fn t09_log_page_has_extras_and_suggestions() {
    let page: LogPage = parse(include_str!("../../tests/fixtures/log_following.json"));
    assert_eq!(page.data.len(), 2);
    assert_eq!(page.meta.next_cursor.as_deref(), Some("cursor-log"));
    assert_eq!(page.log, "following");
    assert!(!page.is_ranked);
    assert_eq!(page.new_commits_count, Some(3));
    assert_eq!(page.suggestions.len(), 1);
    assert_eq!(page.suggestions[0].body, "suggested");
}

#[test]
fn t09_log_page_without_new_commits_count_key_is_none() {
    let mut v: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/log_following.json")).unwrap();
    v.as_object_mut().unwrap().remove("new_commits_count");
    let page: LogPage = parse_value(v);
    assert_eq!(page.new_commits_count, None);
}

#[test]
fn t09_thread_response_has_commit_and_ancestors_with_tombstone() {
    let thread: ThreadResponse = parse(include_str!("../../tests/fixtures/thread.json"));
    assert_eq!(thread.data.replies_count, Some(2));
    assert_eq!(thread.ancestors.len(), 2);
    assert!(matches!(&thread.ancestors[0], ThreadItem::Commit(c) if c.body == "root"));
    assert!(matches!(&thread.ancestors[1], ThreadItem::Tombstone(_)));
}

#[test]
fn t09_me_response_has_current_user_fields_and_unread_count() {
    let me: MeResponse = parse(include_str!("../../tests/fixtures/me.json"));
    assert_eq!(me.unread_notifications_count, 4);
    let user = me.data;
    assert_eq!(user.user.username, "alice");
    assert_eq!(user.user.lane, Some(Lane::Custom(Rgb(0xff, 0x88, 0x00))));
    assert!(user.pasteis_mode);
    assert_eq!(user.pasteis_remaining, 8);
    assert_eq!(user.pasteis_received, 9);
    assert_eq!(user.pasteis_resets_at, datetime!(2026-10-21 00:00:00 UTC));
    assert_eq!(
        user.preferred_lane,
        Some(Lane::Custom(Rgb(0xff, 0x88, 0x00)))
    );
    assert_eq!(user.automatic_lane, 2);
    assert_eq!(user.sponsors_url, None);
    assert_eq!(
        user.github_synced_at,
        Some(datetime!(2026-10-19 08:00:00 UTC))
    );
}

// 10. 未知のフィールドや値

#[test]
fn t10_unknown_fields_are_ignored() {
    let user: User = parse_value(user_json(json!({ "brand_new_field": [1, 2, 3] })));
    assert_eq!(user.username, "alice");
    let commit: Commit = parse_value(commit_json(json!({ "brand_new_field": { "a": 1 } })));
    assert_eq!(commit.body, "hello");
}

#[test]
fn t10_unknown_status_is_unknown() {
    let commit: Commit = parse_value(commit_json(json!({ "status": "flagged" })));
    assert_eq!(commit.status, Some(Status::Unknown));
}

#[test]
fn t10_unknown_segment_type_is_unknown() {
    let commit: Commit = parse_value(commit_json(json!({
        "body_segments": [{ "type": "math", "tex": "x^2" }, { "type": "text", "text": "a" }]
    })));
    assert_eq!(
        commit.body_segments,
        Some(vec![Segment::Unknown, Segment::Text { text: "a".into() }])
    );
}

#[test]
fn t10_unknown_embed_kind_is_unknown() {
    let commit: Commit = parse_value(commit_json(json!({
        "link_preview": {
            "url": "https://example.com", "title": null, "description": null,
            "image_url": null, "site_name": null, "is_pending": false,
            "embed": { "provider": "x", "kind": "hologram", "src": "https://example.com/e",
                       "aspect_ratio": null, "height": null, "poster": null }
        }
    })));
    let embed = commit.link_preview.unwrap().embed.unwrap();
    assert_eq!(embed.kind, EmbedKind::Unknown);
}

// 8b. effective_lane

#[test]
fn t08_effective_lane_uses_own_lane_or_falls_back_to_auto_lane() {
    let with: User = parse_value(user_json(json!({ "lane": 2 })));
    assert_eq!(with.effective_lane(), Lane::Index(2));
    let without: User = parse_value(user_json(json!({ "id": "a" })));
    assert_eq!(without.effective_lane(), Lane::Index(1));
}
