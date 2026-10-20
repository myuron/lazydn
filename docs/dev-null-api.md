# dev-null.sh API リファレンス

[dev-null.sh](https://dev-null.sh)（/dev/null）は、開発者向けの SNS です。投稿は「commit」と呼ばれます。このドキュメントは、lazydn から利用する HTTP API の仕様をまとめたものです。

> **注意**: dev-null.sh は公式の API ドキュメントを公開していません。この内容は、ソースコード [nunomaduro/dev-null.sh](https://github.com/nunomaduro/dev-null.sh)（branch `production`、commit `627d25a2969915fd452e4486884efdac6108b48a`、2026-10-20 時点）を読んで逆引きし、公開エンドポイントは実際にリクエストして確かめたものです。API は予告なく変わる可能性があります。挙動が食い違ったときは、`routes/api.php` と `app/Http/Controllers/Api/` を確認してください。

## 概要

| 項目 | 値 |
| --- | --- |
| Base URL | `https://dev-null.sh/api` |
| 形式 | JSON（`/cli/*` だけは text/plain で応答） |
| ID | commit・user・notification はすべて UUID（v7） |
| 日時 | ISO 8601 の UTC（例: `2026-10-15T13:02:44Z`） |
| バージョン | パスの先頭に `/v1`（`/cli` は付かない） |

API は次の 3 つに分かれます。

1. **公開 API**（`/v1/users/...`、`/v1/commits/{id}`）: 認証なしで読めます。署名を付けると、そのユーザーから見た内容が返ります。
2. **署名付き API**（それ以外の `/v1/*`）: モバイルアプリ用の API です。「デバイス」として承認した SSH 鍵でリクエストに署名します。
3. **CLI API**（`/cli/*`）: `devnull commit` スクリプト用の API です。GitHub に登録した SSH 鍵で署名して投稿します。

## 認証

### ゲスト（認証なし）

公開 API だけが使えます。

- 署名ヘッダー（`X-Devnull-Timestamp`、`X-Devnull-Nonce`、`X-SSH-Signature`）が**1 つでも**付いていれば、署名の検証が必須になります。壊れた署名を送ってもゲスト扱いにはならず、`401 invalid_signature` が返ります。
- ゲストには、承認済み（`approved`）の commit だけが返ります。

### 署名付きリクエスト（`/v1/*`）

アプリ側の SSH 鍵で、リクエストごとに `ssh-keygen -Y sign` 形式（SSHSIG）の署名を付けます。

| 項目 | 内容 |
| --- | --- |
| 名前空間 | `devnull-api` |
| 署名するメッセージ | `"<unix timestamp>\n<nonce>\n<METHOD>\n<path と query>\n<生のボディ>"` |
| `X-Devnull-Timestamp` | Unix 秒。サーバー時刻との差は ±300 秒まで |
| `X-Devnull-Nonce` | 小文字 hex 32 文字。使えるのは 1 回だけ |
| `X-SSH-Signature` | SSHSIG の armored 署名（`-----BEGIN SSH SIGNATURE-----` …） |

- `<path と query>` は `/api/v1/me` や `/api/v1/log/latest?since=...` のように、`/api` から始まるリクエスト URI です（`Request::getRequestUri()`）。
- ボディは **JSON に限ります**。multipart やフォーム形式は拒否されます。ボディがなければ空文字列を署名します。
- 鍵は、Web 上でユーザーが「デバイス」として承認している必要があります。GitHub アカウントに登録された鍵である必要はなく、逆に GitHub に登録しただけの鍵では署名付き API を使えません。
- 署名が通ると、ユーザー単位のレート制限（120 回/分）が適用されます。署名に失敗したリクエストは、IP ごとに 60 回/分まで受け付けます。

**送るべきとき**: 公開 API 以外を呼ぶとき。公開 API でも、ログインユーザーから見た内容（`lane`、`is_followed`、ブロック状態など）がほしいとき。

### デバイス登録（サインイン）フロー

OAuth の Authorization Code + PKCE（RFC 7636）と同じ流れです。

1. アプリが SSH 鍵ペアと PKCE の `code_verifier` / `code_challenge` を作ります。
2. ユーザーが Web 上でアプリを承認すると、その鍵の fingerprint に結び付いた `code` が渡されます（Web 側の承認は 10 回/分まで）。
3. アプリは、承認してほしい鍵で署名して `POST /v1/devices` を呼びます。

```http
POST /api/v1/devices
Content-Type: application/json
X-Devnull-Timestamp: 1760000000
X-Devnull-Nonce: 0123456789abcdef0123456789abcdef
X-SSH-Signature: -----BEGIN SSH SIGNATURE-----...

{"code": "<Web で受け取った code>", "code_verifier": "<43〜128 文字の [A-Za-z0-9._~-]>"}
```

- 成功すると、`GET /v1/me` と同じ形のレスポンスが返り、以後その鍵で署名付き API を使えます。
- code が不明・期限切れ・使用済み、別の鍵用、verifier が一致しない、ユーザーが削除済み、のどれであっても、`400 invalid_grant` が返ります。
- 制限は IP ごとに 60 回/分です。署名が通った後は、さらに鍵ごとに 12 回/分の制限がかかります。

### CLI 署名（`/cli/*`）

`devnull commit` スクリプトの方式です。GitHub アカウントに登録した SSH 鍵（dev-null.sh 側で同期済みのもの）で、ペイロード全体に署名します。

| 項目 | 内容 |
| --- | --- |
| 名前空間 | `devnull-commit`（画像付きのときは `devnull-commit-images`） |
| ペイロード（画像なし） | `"<unix ts>\n<nonce>\n<返信先 or 空>\n<本文>"` |
| ペイロード（画像あり） | `"<unix ts>\n<nonce>\n<返信先 or 空>\n<各画像の sha256 hex をカンマ区切り>\n<本文>"` |
| ヘッダー | `X-SSH-Signature` |

- timestamp（±300 秒）と nonce（hex 32 文字・1 回限り）のルールは、署名付き API と同じです。
- 鍵が GitHub から削除されると、数分以内に使えなくなります。
- 失敗すると、`401` と plain text のメッセージが返ります。

## エンドポイント一覧

認証の列: **公開** = 署名は任意、**署名** = 署名付きリクエストが必須、**CLI** = CLI 署名。
レート制限の列が空欄のものには、[共通の制限](#レート制限) だけが適用されます。

| メソッド | パス | 認証 | 追加のレート制限 | 内容 |
| --- | --- | --- | --- | --- |
| GET | `/v1/users/{username}` | 公開 | | ユーザーのプロフィール |
| GET | `/v1/users/{username}/commits` | 公開 | | ユーザーの commit 一覧 |
| GET | `/v1/users/{username}/followers` | 公開 | | フォロワー一覧 |
| GET | `/v1/users/{username}/following` | 公開 | | フォロー中の一覧 |
| GET | `/v1/commits/{id}` | 公開 | | commit 1 件 |
| POST | `/v1/devices` | 署名（未承認の鍵） | IP 60/分、鍵 12/分 | デバイス登録 |
| GET | `/v1/me` | 署名 | | 自分の情報 |
| DELETE | `/v1/me` | 署名 | | アカウント削除 |
| PUT | `/v1/me/pasteis-mode` | 署名 | | pastéis モードの切り替え |
| PUT | `/v1/me/github` | 署名 | 1/分 | GitHub からプロフィールを再同期 |
| PUT | `/v1/me/lane` | 署名 | | 表示色（lane）の変更 |
| GET | `/v1/me/muted` | 署名 | | ミュート中のユーザー |
| GET | `/v1/me/blocked` | 署名 | | ブロック中のユーザー |
| GET | `/v1/log/{log}` | 署名 | | タイムライン |
| POST | `/v1/commits` | 署名 | 10/分 | commit の投稿 |
| DELETE | `/v1/commits/{id}` | 署名 | | commit の削除 |
| GET | `/v1/commits/{id}/replies` | 署名 | | 返信一覧 |
| POST | `/v1/commits/{id}/reports` | 署名 | 5/分 | 通報 |
| POST | `/v1/commits/{id}/pasteis` | 署名 | 60/分 | pastel を贈る |
| GET | `/v1/saved` | 署名 | | 保存した commit |
| PUT | `/v1/commits/{id}/bookmark` | 署名 | 60/分 | 保存 |
| DELETE | `/v1/commits/{id}/bookmark` | 署名 | 60/分 | 保存の解除 |
| POST | `/v1/commits/{id}/star` | 署名 | 60/分 | star |
| DELETE | `/v1/commits/{id}/star` | 署名 | 60/分 | star の解除 |
| POST | `/v1/users/{username}/follow` | 署名 | 30/分 | フォロー |
| DELETE | `/v1/users/{username}/follow` | 署名 | 30/分 | フォローの解除 |
| POST | `/v1/users/{username}/mute` | 署名 | | ミュート |
| DELETE | `/v1/users/{username}/mute` | 署名 | | ミュートの解除 |
| POST | `/v1/users/{username}/block` | 署名 | | ブロック |
| DELETE | `/v1/users/{username}/block` | 署名 | | ブロックの解除 |
| GET | `/v1/notifications` | 署名 | | 通知一覧 |
| GET | `/v1/notifications/{id}/group` | 署名 | | 通知グループの中身 |
| POST | `/cli/key-lookups` | なし | 60/分 | 投稿に使える鍵を探す |
| POST | `/cli/commits` | CLI | 10/分 | CLI からの投稿 |

## エンドポイント詳細

### 公開 API

#### `GET /v1/users/{username}`

ユーザーのプロフィールを返します。`commits_count`、`followers_count`、`following_count` も含まれます。

- **使うとき**: プロフィール画面を表示するとき。
- **失敗するとき**: ユーザーがいなければ `404`。署名付きでブロック関係にあるときは、`403 user_blocked`（自分がブロックした側で、`user` フィールドが付く）または `403 user_unavailable`。

```json
{"data": {"id": "01a13fa8-...", "username": "nunomaduro", "name": "nunomaduro", "avatar_url": "https://avatars.githubusercontent.com/...", "bio": "...", "company": "@laravel", "location": "Portugal", "website": "https://...", "github_url": "https://github.com/nunomaduro", "html_url": "https://dev-null.sh/@nunomaduro", "feed_url": "https://dev-null.sh/@nunomaduro/feed.atom", "created_at": "2026-10-15T13:02:44Z", "commits_count": 187, "followers_count": 3647, "following_count": 87}}
```

#### `GET /v1/users/{username}/commits`

ユーザーの commit を新しい順に返します。1 ページ 20 件です。

| クエリ | 内容 |
| --- | --- |
| `replies` | `true` にすると、トップレベルの投稿ではなく返信のタブを返します |
| `cursor` | ページネーション用 |

- ゲストには、承認済みの commit だけが返ります。署名付きなら、Web でそのユーザーに見える内容（保留中の自分の commit、`reply_preview` など）が返ります。

#### `GET /v1/users/{username}/followers`、`GET /v1/users/{username}/following`

フォロワー、またはフォロー中のユーザーを返します。1 ページ 30 件です。署名付きなら、各ユーザーに `is_followed`（自分がフォローしているか）と `follows_you`（相手が自分をフォローしているか）が付きます。

#### `GET /v1/commits/{id}`

commit を 1 件返します。

- ゲストには、`link_preview` と `images` が付いた公開版が返ります。
- 署名付きなら、commit ページに表示される完全版に加えて、トップレベルの `ancestors`（返信元の commit を上から順に並べたもの）が返ります。
- **失敗するとき**: 存在しない、または見られないときは `404`（ゲストには Laravel 標準の `{"message": "No query results for model ..."}`）。

### アカウント

#### `GET /v1/me`

自分の情報を返します。[CurrentUser](#currentuser) の形に、トップレベルの `unread_notifications_count` が付きます。

- **使うとき**: 起動直後のセッション確認、通知バッジの更新。

#### `DELETE /v1/me`

アカウントとすべてのデバイスを削除します。`204` が返り、以後その鍵は使えなくなります。

#### `PUT /v1/me/pasteis-mode`

```json
{"enabled": true}
```

pastéis モードのオン・オフを切り替えます。`GET /v1/me` と同じ形で応答します。

#### `PUT /v1/me/github`

プロフィールを GitHub から同期し直します。ユーザーごとに 1 回/分までです。超えると `429 rate_limited`（`You just synced. Try again in a minute.`）、GitHub 側で失敗すると `503 github_sync_failed` が返ります。

#### `PUT /v1/me/lane`

```json
{"lane": 3}
{"lane": null}
{"lane": null, "color": "#ff8800"}
```

- `lane`: 必須（`null` も可）。`0`〜`5` の整数、または `null`（Automatic）。
- `color`: 任意。小文字の `#rrggbb`。`lane` と同時には指定できません。
- `GET /v1/me` と同じ形で応答します。

#### `GET /v1/me/muted`、`GET /v1/me/blocked`

ミュート中、またはブロック中のユーザーの一覧を返します。

### タイムライン

#### `GET /v1/log/{log}`

`{log}` には `following`、`latest`、`trending` のどれかを指定します。1 ページ 20 件です。返すのはトップレベルの commit（`parent_id` が null のもの）だけで、返信は含まれません。

| クエリ | 内容 |
| --- | --- |
| `since` | 最後に見た commit の UUID。指定すると、それより新しい commit の数を `new_commits_count` として返します |
| `cursor` | ページネーション用 |

一覧のほかに、次のフィールドがトップレベルに付きます。

| フィールド | 内容 |
| --- | --- |
| `log` | 要求したログの名前 |
| `is_ranked` | `trending` のときだけ `true`。新しい順ではないので、`new_commits_count` は意味を持ちません |
| `new_commits_count` | `since` を指定したときだけ付きます |
| `suggestions` | `following` で誰もフォローしていないときのおすすめ commit。それ以外は空配列 |

- **使うとき**: メイン画面のフィード。`since` を指定してポーリングすれば、「新着 N 件」を表示できます。

### Commit

#### `POST /v1/commits`

```json
{"body": "hello /dev/null", "parent_id": null, "quoted_commit_id": null}
```

| フィールド | 内容 |
| --- | --- |
| `body` | 必須。1〜4000 文字。連続する空行は、長さを測る前に詰められます |
| `parent_id` | 返信先の commit の UUID（任意） |
| `quoted_commit_id` | 引用する commit の UUID（任意）。`parent_id` と同時には指定できません |

- `201` で、作成された commit が返ります。管理者以外の commit は `status: "pending"`（レビュー待ち）になり、承認されるまでは本人にしか見えません。
- 署名付き API では、画像と投票はまだ付けられません。
- **失敗するとき**: バリデーションエラーは `422`。返信先が見えなくなっていれば `404`。投稿は CLI と合わせて 10 回/分までで、超えると `429 rate_limited`。

#### `DELETE /v1/commits/{id}`

自分の commit を削除します。`204` が返ります。返信が付いている commit は、スレッド上に [tombstone](#tombstone削除済み-commit) として残ります。

#### `GET /v1/commits/{id}/replies`

返信の一覧を返します。削除済みの commit の返信も取得できます。

| クエリ | 内容 |
| --- | --- |
| `newest` | `true` にすると新しい順で返します。`reply_preview.next_cursor` の続きを取るときに使います |
| `cursor` | ページネーション用 |

#### `POST /v1/commits/{id}/reports`

```json
{"reason": "spam", "details": "任意の補足（500 文字まで）"}
```

- `reason`: `spam`、`harassment`、`unsafe`、`other` のどれか。
- `204` が返ります。同じ commit をもう一度通報しても、最初の通報が残ります。
- **失敗するとき**: 承認済みでない、削除済み、見られない commit は `404 commit_unavailable`。自分の commit は `403 own_commit`。

#### `POST /v1/commits/{id}/pasteis`

1 日の割り当てから pastel を 1 つ贈ります。更新後の commit に加えて、`pasteis_remaining`、`pasteis_resets_at`、`author_pasteis_received` がトップレベルに付きます。その日の割り当てを使い切っていると、`422 pasteis_exhausted` が返ります。

#### `GET /v1/saved`、`PUT|DELETE /v1/commits/{id}/bookmark`

保存した commit を、最後に保存した順に返します（1 ページ 20 件）。PUT と DELETE は、`is_saved` を更新した commit を返します。

#### `POST|DELETE /v1/commits/{id}/star`

commit に star を付ける、または外します。

### ユーザー操作

#### `POST|DELETE /v1/users/{username}/follow|mute|block`

フォロー、ミュート、ブロックと、それぞれの解除です。ブロックすると、互いにフォロー・返信・star ができなくなり、互いの commit も見えなくなります。

### 通知

#### `GET /v1/notifications`

通知をグループ単位で返します。

| クエリ | 内容 |
| --- | --- |
| `type` | `all`、`none`、または `replies`、`quotes`、`stars`、`mentions`、`follows`、`moderation` のカンマ区切り。指定した値は Cookie に保存され、省略したときに使われます |
| `through` | 日時。指定した時点までの通知を返します |

- **呼ぶと、フィルターにかかわらず全通知が既読になります。** トップレベルの `read_at` には、既読にした時刻が入ります。応答の各通知の `read_at` は既読にした後の値です。そのため、`read_at` が null か、トップレベルの `read_at` 以上のものが今回の新着になります（Web 版の `isNewSince` と同じ規則）。一覧の `unread_count` は既読にした後に数えるので、常に 0 です。
- トップレベルに `filters.type` も付きます。

#### `GET /v1/notifications/{id}/group`

指定した通知が属するグループの通知を、すべて新しい順に返します（1 ページ 20 件。`type` で絞り込めます）。こちらは既読にしません。

### CLI

#### `POST /cli/key-lookups`

ボディには、OpenSSH 形式の公開鍵を改行区切りで最大 20 個入れます（text/plain）。dev-null.sh のユーザーに属する鍵が見つかれば、その 1 本を text/plain で返します。見つからなければ `404` が返ります。データベースを参照するだけで、GitHub には問い合わせません。

- **使うとき**: ssh-agent に複数の鍵があるとき、署名に使う鍵を選ぶため。

#### `POST /cli/commits`

- 画像なしのとき: ボディに署名済みペイロードをそのまま送ります。
- 画像ありのとき: multipart で `payload`（ペイロードのファイル）と `images[]`（最大 3 枚、jpeg・png・webp、各 10MB まで）を送ります。
- 返信先は UUID ではなく、commit のハッシュかリンクで指定します。
- `201` で `Committed: <url>` の text/plain が返ります（保留中のときは注記付き）。投稿の上限は 10 回/分で、超えると `429` と plain text のメッセージが返ります。

## レスポンス形式

### ページネーション

一覧はすべて cursor ページネーションです。次のページは `links.next` の URL、または `?cursor=<meta.next_cursor>` で取得します。`next_cursor` が `null` なら最後のページです。

```json
{
  "data": [ ... ],
  "links": {"first": null, "last": null, "prev": null, "next": "https://dev-null.sh/api/v1/users/x/followers?cursor=..."},
  "meta": {"path": "...", "per_page": 30, "next_cursor": "eyJ...", "prev_cursor": null}
}
```

追加のフィールド（`log`、`read_at` など）は、`data` と同じ階層に入ります。

### User

| フィールド | 型 | 内容 |
| --- | --- | --- |
| `id` | string | UUID |
| `username` | string | GitHub のユーザー名 |
| `name`, `avatar_url`, `bio`, `company`, `location`, `website` | string \| null | GitHub から同期した値 |
| `github_url`, `html_url`, `feed_url` | string | 各 URL（`feed_url` は Atom） |
| `created_at` | string | 登録日時 |
| `commits_count`, `followers_count`, `following_count` | int | 件数を読み込んだときだけ付きます |
| `lane` | int \| string | 署名付きのときだけ付きます。表示色 |
| `is_followed`, `follows_you` | bool | 署名付きのユーザー一覧でだけ付きます |

### CurrentUser

[User](#user) のフィールドに、次が加わります。

| フィールド | 内容 |
| --- | --- |
| `pasteis_mode` | pastéis モードがオンか |
| `pasteis_remaining` | 今日残っている pastel の数 |
| `pasteis_resets_at` | 割り当てが戻る日時（翌日 0 時） |
| `pasteis_received` | これまでに受け取った pastel の数 |
| `preferred_lane` | 選んだ lane または色。Automatic なら `null` |
| `automatic_lane` | Automatic のときに使われる lane |
| `sponsors_url` | GitHub Sponsors の URL。なければ `null` |
| `github_synced_at` | GitHub と最後に同期した日時 |

### Commit

公開版（ゲストへの応答）のフィールド:

| フィールド | 型 | 内容 |
| --- | --- | --- |
| `id` | string | UUID |
| `short_hash` | string | UUID のハイフンを除いた末尾 7 文字 |
| `body` | string | 本文（Markdown 風のテキスト） |
| `parent_id` | string \| null | 返信先の ID |
| `html_url` | string | Web 上の URL |
| `created_at` | string | 投稿日時 |
| `stars_count`, `replies_count` | int | 読み込んだときだけ付きます |
| `link_preview` | object \| null | `url`, `title`, `description`, `image_url`, `site_name`, `is_pending`, `embed` |
| `images` | array | `id`, `width`, `height`, `description`, `url` |
| `author` | [User](#user) | 投稿者 |

署名付きのときは、さらに次が加わります。

| フィールド | 内容 |
| --- | --- |
| （本文の分割結果） | 本文を地の文・インラインコード・ハイライト済みのコードブロックに分けたもの。`CommitBodies::segments()` の出力で、一覧用の抜粋 `preview_segments` も含みます |
| `mentions` | 本文中のメンションの位置 |
| `status` | `pending`、`approved`、`rejected` |
| `is_pinned` | 投稿者がピン留めしているか |
| `starred`, `is_saved` | 自分が star・保存しているか |
| `pasteis_count` | 受け取った pastel の数 |
| `deleted_at` | 常に `null` |
| `parent` | 返信先の Commit（読み込んだときだけ） |
| `quoted_commit_id`, `quoted_commit` | 引用先。見られなくなった引用先は `quoted_commit: null` になりますが、ID は残ります |
| `poll` | [Poll](#poll) |
| `reply_preview` | 一覧に表示する最新の返信。`{data: Commit[], next_cursor}` |

#### Tombstone（削除済み commit）

返信が付いた削除済みの commit は、スレッド上に `{id, parent_id, deleted_at, replies_count}` だけの形で残ります。author と body は含まれません。署名付きのリクエストにだけ返ります。

### Poll

| フィールド | 内容 |
| --- | --- |
| `id`, `closes_at`, `is_closed`, `has_voted` | 基本情報 |
| `total_votes` | 投票した・自分の投票である・締め切った、のどれかを満たすまでは `null` |
| `options[]` | `id`, `label`, `votes_count`（同上の条件まで `null`）, `voted`, `percentage` |

投票期間は 60、1440、4320、10080 分（1 時間・1 日・3 日・7 日）のどれかです。

### Notification

| フィールド | 内容 |
| --- | --- |
| `id`, `group_id` | 通知 ID と、所属するグループの ID |
| `type` | `followed`、`replied`、`mentioned`、`approved`、`rejected`、`quoted`、`starred` |
| `subject_id` | 対象の ID |
| `reason` | commit が却下されたときの理由 |
| `read_at`, `created_at` | 日時 |
| `actor` / `actors` | 最新のアクター / 一覧では先頭の数人（[User](#user)） |
| `event_count`, `actors_count`, `unread_count` | グループ内のイベント数・アクター数・未読数 |
| `commit` | 関係する Commit（返信先付き）、または `null` |

## エラー

署名付き API のエラーは、次の形で返ります。`code` は一度リリースされたら変わらないので、分岐には `message` ではなく `code` を使ってください。

```json
{"message": "This commit isn't available.", "code": "commit_unavailable"}
```

| code | HTTP | 発生するとき |
| --- | --- | --- |
| `invalid_grant` | 400 | デバイス登録の code が使えない |
| `invalid_signature` | 401 | 署名が不正、期限切れ（時刻ずれ）、nonce の再利用、ボディが JSON でない |
| `device_not_approved` | 401 | 署名は正しいが、鍵がデバイスとして承認されていない |
| `user_blocked` | 403 | 自分がブロックした相手を見ようとした（`user` フィールドが付く） |
| `user_unavailable` | 403 | 相手にブロックされている |
| `own_commit` | 403 | 自分の commit を通報しようとした |
| `commit_unavailable` | 404 | commit がない、削除済み、未公開、または見られない（区別しません） |
| `pasteis_exhausted` | 422 | その日の pastel を使い切った |
| `rate_limited` | 429 | レート制限。`Retry-After` ヘッダーが付く |
| `github_sync_failed` | 503 | GitHub との同期に失敗した |

上の表にないエラーは、Laravel 標準の形で返ります。

- バリデーションエラー: `422 {"message": "...", "errors": {"field": ["..."]}}`
- モデルが見つからない: `404 {"message": "No query results for model ..."}`
- ゲストへのレート制限: `429 {"message": "Too Many Attempts."}`（`code` なし）

## レート制限

応答には `X-RateLimit-Limit` と `X-RateLimit-Remaining` が付きます。超過したときは、`Retry-After`（秒）が付きます。

| 対象 | 上限 | 単位 |
| --- | --- | --- |
| `/v1/*` 共通（ゲスト） | 60/分 | IP |
| `/v1/*` 共通（署名付き） | 120/分 | ユーザー |
| 署名の検証失敗 | 60/分 | IP |
| `POST /v1/devices` | 60/分 + 12/分 | IP + 鍵 |
| commit の投稿（API と CLI の合計） | 10/分 | ユーザー |
| star / bookmark | 60/分 | ユーザー |
| follow | 30/分 | ユーザー |
| report | 5/分 | ユーザー |
| pastel | 60/分 | ユーザー |
| GitHub の再同期 | 1/分 | ユーザー |
| `/cli/key-lookups` | 60/分 | IP |

## lazydn での使い方

- **読み取りだけ**なら、公開 API で足ります。プロフィール、ユーザーの commit、フォロー関係、commit 1 件を取得できます。ただし、タイムライン（`/v1/log/*`）、返信一覧、通知には署名が必要です。
- **フル機能**を使うには、SSH 鍵を作り、デバイス登録フローで承認を受けたうえで、全リクエストに署名します。承認画面は Web 側の `GET /mobile/authorize` で、パラメータは `fingerprint`、`device_name`（100 文字まで）、`code_challenge`、`code_challenge_method=S256`、`state`、`redirect_uri` です。`redirect_uri` は `devnull://` で始まる必要があり、承認すると `?code=<64 文字の 16 進数>&state=…` 付きでそこへリダイレクトされます（キャンセルなら `error=access_denied`）。code の有効期限は 5 分で、1 回だけ使えます。
- **投稿だけ**なら、GitHub に登録済みの SSH 鍵を使って CLI 方式（`/cli/key-lookups` → `/cli/commits`）でもできます。デバイス登録は必要ありません。
- 署名には `ssh-keygen -Y sign -n <namespace>` 相当の SSHSIG 形式を使います。Rust なら `ssh-key` crate の `SshSig` で作れます。
