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
| `?` | Help |
| `Q` | Quit |

While you write (`c`, `r` or `q`), every key goes into the text: `Ctrl+Enter` or `Ctrl+S` posts and `Esc` closes. The draft is kept.

## Development

`just check` runs formatting, clippy and the tests. See [CONTRIBUTING.md](CONTRIBUTING.md).

The automated tests never connect to dev-null.sh: they use local mock servers and temporary directories, and start no browser.
