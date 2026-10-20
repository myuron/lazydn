# lazydn

A terminal UI client for [dev-null.sh](https://dev-null.sh), built with Rust and Ratatui.

lazydn is work in progress. You can read the log, threads, profiles and notifications, write commits (`c`), and reply to (`r`) or quote (`q`) a commit. Stars and the like are not available yet.

## Start

Build and run with a stable Rust toolchain:

```sh
cargo run --release
```

Without a signed-in device, lazydn starts as a guest on the Latest log. Press `?` for the key list.

### Sign in

1. Press `L` to open the sign-in dialog. The first time, press `Enter` to create the device key.
2. Press `Ctrl+O` to open the approval page in your browser, or `Ctrl+Y` to copy its URL. The copy is sent to the terminal with OSC 52; inside tmux, `set -g set-clipboard on` is needed, and the outer terminal must support OSC 52 too; otherwise the copy is silently lost.
3. Approve the device on the page. The browser then tries to open a `devnull://lazydn/callback?code=...` URL that no program handles.
4. Copy that whole `devnull://...` URL (or only its `code`) and paste it into the Code field, then press `Enter`.
   - Chromium-based browsers show "Open xdg-open?" and keep the URL out of the address bar. Open the developer tools before approving, enable **Network → Preserve log**, press Approve, select the `authorize` request (POST, status 303) and copy its `location` response header.
   - Firefox usually leaves the `devnull://` URL in the address bar of its error page.
   - The code expires after five minutes and works once. Keep the dialog's URL (do not press `Ctrl+R`) and approve again if it expires.

The browser is opened only when you press `Ctrl+O`.

### Files

| File | Place |
| --- | --- |
| `id_ed25519` (device key, mode 600) | config directory |
| `config.toml` (appearance) | config directory |
| `state.toml` (last visit per log tab) | state directory |
| `drafts-<account id>.toml` (unsent commit text, mode 600) | state directory |

The config directory is `$LAZYDN_CONFIG_DIR`, else `$XDG_CONFIG_HOME/lazydn`, else `~/.config/lazydn`.
The state directory is `$LAZYDN_STATE_DIR`, else `$XDG_STATE_HOME/lazydn`, else `~/.local/state/lazydn`.
A relative `XDG_*` value is ignored. A relative `LAZYDN_CONFIG_DIR` or `LAZYDN_STATE_DIR` stops lazydn at startup. An empty variable counts as unset.

lazydn only reads `config.toml`; create it yourself. Keys: `theme` (`"dark"` or `"light"`), `collapse_nav`, `lane_dimming`, `ascii` (booleans) and `reply_preview_limit` (1, 3, 5, 10, 15, 20 or 25). A field that is missing falls back to its default; one with the wrong type or a value that is not allowed also falls back, and is reported in a toast.

The last visit is saved when you quit with `Q` or `Ctrl+C`; SIGTERM and SIGHUP (closing the terminal) do not save it.

Drafts are saved about two seconds after you stop typing, and at once when you close the composer with `Esc`, post a commit or quit. Post with `Ctrl+Enter` where the terminal can tell it from `Enter` (e.g. kitty, foot, Ghostty, WezTerm), and with `Ctrl+S` everywhere.

### Environment

| Variable | Meaning |
| --- | --- |
| `LAZYDN_CONFIG_DIR` | Config directory, holding the device key. |
| `LAZYDN_STATE_DIR` | State directory. |
| `LAZYDN_BASE_URL` | API host instead of `https://dev-null.sh`. Only `https`, or `http` to localhost, 127.0.0.1 (any 127.0.0.0/8 address) or ::1, is accepted; anything else, including a URL with a user name, a path, a query, a fragment or surrounding spaces, stops lazydn at startup. An empty value counts as unset. The status line shows the host when it is not the default. |
| `COLORTERM` | `truecolor` or `24bit` turns on 24-bit colors. |

### Keys

| Key | Action |
| --- | --- |
| `j` / `k` | Next / previous item |
| `Enter`, `Esc` | Open, back |
| `1`-`3` | Switch tab |
| `g` then `l` `e` `n` `p` `s` `v` | Log, Trending, Notifications, Profile, Settings, Saved |
| `R` | Refresh |
| `c` | Write a commit |
| `r` | Reply to the selected commit, in a box under its card |
| `q` | Quote the selected commit, in a dialog |
| `s` | Star or unstar the selected commit (not your own) |
| `b` | Save or unsave the selected commit |
| `p` | Give a pastel to the selected commit (only with the pastéis mode on, not to your own) |
| `y` | Copy the link of the selected commit |
| `O` | Open the selected commit in the browser; on a profile with no commit, open the user's page |
| `.` | More menu of the selected commit: every action that applies to it (`j` / `k` choose, `Enter` or the action's key runs it, `Esc` closes) |
| `d` | Delete your own commit, after a confirmation (`y` deletes, `n` or `Esc` cancels) |
| `!` | Report the selected commit (not your own): `j` / `k` choose the reason, `Tab` moves to the details, `Enter` sends |
| `f` | Follow or unfollow the person on a profile, or on the selected row of Followers / Following |
| `m` | Mute or unmute the person on a profile |
| `B` | Block the person on a profile, after a confirmation (`y` blocks, `n` or `Esc` cancels); on a blocked profile, unblock |
| `S` | Sync your profile from GitHub (Settings → Profile) |
| `D` | Delete your account, after typing your username (Settings → Profile) |
| `Left` / `Right`, `h` / `l` | Move over the lane colors (Settings → Profile); `Enter` saves the choice, and on `custom` it opens a field for six hex digits |
| `Space` | Turn the pastéis mode on or off (Settings → Profile; `Enter` does the same) |
| `u` / `Enter` | Unmute or unblock the selected person (Settings → Safety) |
| `?` | Help |
| `Q` | Quit |

While you write (`c`, `r` or `q`), every key goes into the text: `Ctrl+Enter` or `Ctrl+S` posts and `Esc` closes. The draft is kept.

In Settings, a guest can use the Appearance tab; the Profile and Safety tabs show a sign-in hint, and their write keys open the sign-in dialog. Typing a lane color or your username for the account deletion also sends every key into the field, except `Esc`, `Enter`, `Backspace` and `Ctrl+C`.

Deleting the account (`D`, then your username exactly as it is written, then `Enter`) cannot be undone. It turns you into a guest, but **it does not delete anything on your machine**: the device key, the drafts and `state.toml` stay in place, and the server simply stops accepting the key. Remove them yourself if you want to. If the answer is lost (a timeout), lazydn says it could not confirm the deletion; check dev-null.sh before trying again.

The Safety tab lists only the first page (20 people) of the muted and the blocked users, so the 21st and later cannot be seen or released from lazydn.

## Development

`just check` runs formatting, clippy and the tests. See [CONTRIBUTING.md](CONTRIBUTING.md).

The automated tests never connect to dev-null.sh: they use local mock servers and temporary directories, and start no browser.
