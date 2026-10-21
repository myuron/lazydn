# lazydn

A terminal UI client for [dev-null.sh](https://dev-null.sh), the social network for developers. Built with Rust and [Ratatui](https://ratatui.rs).

## Features

- Read the Latest, Following and Trending logs, threads, profiles, notifications and saved commits.
- Write commits, reply to them in place and quote them. Drafts are kept on disk until you post.
- Star, save, give pastéis, copy links, open in the browser, delete your own commits and report others' commits.
- Follow, mute and block people, and release muted or blocked people from Settings.
- Change your profile settings: sync from GitHub, lane color, pastéis mode, account deletion.
- Appearance settings saved to `config.toml`, including an ASCII-only mode for terminals without Unicode box drawing.
- Works as a guest without signing in. Signing in creates an ed25519 device key that you approve in the browser.

## Install

### Prebuilt binary

Releases provide x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu and aarch64-apple-darwin builds. Replace `vX.Y.Z` and `<target>` with the release tag and your target:

```sh
base=https://github.com/myuron/lazydn/releases/download/vX.Y.Z
curl -LO "$base/lazydn-vX.Y.Z-<target>.tar.gz"
curl -LO "$base/lazydn-vX.Y.Z-<target>.tar.gz.sha256"
sha256sum -c lazydn-vX.Y.Z-<target>.tar.gz.sha256   # macOS: shasum -a 256 -c ...
tar xzf lazydn-vX.Y.Z-<target>.tar.gz
mkdir -p ~/.local/bin
install -m 755 lazydn-vX.Y.Z-<target>/lazydn ~/.local/bin/
```

Make sure `~/.local/bin` (or the directory you chose) is on your `PATH`.

### Cargo

```sh
cargo install --locked lazydn
```

### Nix

Flakes must be enabled. Pin a release by appending its tag.

```sh
nix run github:myuron/lazydn
nix run github:myuron/lazydn/vX.Y.Z
nix profile install github:myuron/lazydn
```

### From source

lazydn needs a stable Rust toolchain (edition 2024). From a checkout:

```sh
cargo install --locked --path .
```

## Quick start

1. Start `lazydn`. Without a signed-in device, it opens the Latest log as a guest.
2. Press `?` for the list of keys, and `j` / `k` / `Enter` / `Esc` to move around.
3. Press `L` to sign in (see below) to write, star, follow and so on.
4. Press `Q` to quit.

> **Note:** every write action (posting, starring, following, deleting, account deletion and so on) is sent to the real dev-null.sh at once. Set `LAZYDN_BASE_URL` to try lazydn against another server.

### Sign in

1. Press `L` to open the sign-in dialog. The first time, press `Enter` to create the device key.
2. Press `Ctrl+O` to open the approval page in your browser, or `Ctrl+Y` to copy its URL. The copy is sent to the terminal with OSC 52; inside tmux, `set -g set-clipboard on` is needed, and the outer terminal must support OSC 52 too; otherwise the copy is silently lost.
3. Approve the device on the page. The browser then tries to open a `devnull://lazydn/callback?code=...` URL that no program handles.
4. Copy that whole `devnull://...` URL (or only its `code`) and paste it into the Code field, then press `Enter`.
   - Chromium-based browsers show "Open xdg-open?" and keep the URL out of the address bar. Open the developer tools before approving, enable **Network → Preserve log**, press Approve, select the `authorize` request (POST, status 303) and copy its `location` response header.
   - Firefox usually leaves the `devnull://` URL in the address bar of its error page.
   - The code expires after five minutes and works once. Keep the dialog's URL (do not press `Ctrl+R`) and approve again if it expires.

The browser is opened only when you press `Ctrl+O`.

## Keys

### Moving around

| Key | Action |
| --- | --- |
| `j` / `k` | Next / previous item |
| `Enter`, `Esc` | Open, back |
| `1`-`3` | Switch tab |
| `g` then `l` `e` `n` `p` `s` `v` | Log, Trending, Notifications, Profile, Settings, Saved |
| `R` | Refresh |
| `L` | Sign in |
| `?` | Help |
| `Q` | Quit |

### Writing

| Key | Action |
| --- | --- |
| `c` | Write a commit |
| `r` | Reply to the selected commit, in a box under its card |
| `q` | Quote the selected commit, in a dialog |
| `Ctrl+Enter`, `Ctrl+S` | Post |
| `Esc` | Close the composer; the draft is kept |

While you write, every key goes into the text. `Ctrl+Enter` works only where the terminal can tell it from `Enter` (e.g. kitty, foot, Ghostty, WezTerm); `Ctrl+S` works everywhere.

### Commit actions

| Key | Action |
| --- | --- |
| `s` | Star or unstar the selected commit (not your own) |
| `b` | Save or unsave the selected commit |
| `p` | Give a pastel to the selected commit (only with the pastéis mode on, not to your own) |
| `y` | Copy the link of the selected commit |
| `O` | Open the selected commit in the browser; on a profile with no commit, open the user's page |
| `.` | More menu: every action that applies to the selected commit (`j` / `k` choose, `Enter` or the action's key runs it, `Esc` closes) |
| `d` | Delete your own commit, after a confirmation (`y` deletes, `n` or `Esc` cancels) |
| `!` | Report the selected commit (not your own): `j` / `k` choose the reason, `Tab` moves to the details, `Enter` sends |

### People

| Key | Action |
| --- | --- |
| `f` | Follow or unfollow the person on a profile, or on the selected row of Followers / Following |
| `m` | Mute or unmute the person on a profile |
| `B` | Block the person on a profile, after a confirmation (`y` blocks, `n` or `Esc` cancels); on a blocked profile, unblock |

### Settings

| Key | Where | Action |
| --- | --- | --- |
| `S` | Profile | Sync your profile from GitHub |
| `Left` / `Right`, `h` / `l` | Profile | Move over the lane colors; `Enter` saves the choice, and on `custom` it opens a field for six hex digits |
| `Space`, `Enter` | Profile | Turn the pastéis mode on or off |
| `D` | Profile | Delete your account, after typing your username |
| `u`, `Enter` | Safety | Unmute or unblock the selected person |

A guest can use the Appearance tab; the Profile and Safety tabs show a sign-in hint, and their write keys open the sign-in dialog. Typing a lane color or your username for the account deletion sends every key into the field, except `Esc`, `Enter`, `Backspace` and `Ctrl+C`.

**Deleting the account cannot be undone.** It runs only after you type your username exactly as it is written and press `Enter`. It turns you into a guest, but it does not delete anything on your machine: the device key, the drafts and `state.toml` stay in place, and the server simply stops accepting the key. Remove them yourself if you want to. If the answer is lost (a timeout), lazydn says it could not confirm the deletion; check dev-null.sh before trying again.

## Configuration

### config.toml

`config.toml` in the config directory is optional: lazydn runs with the defaults when it is missing.

| Key | Values | Effect |
| --- | --- | --- |
| `theme` | `"dark"`, `"light"` | Saved, not applied yet |
| `collapse_nav` | boolean | Saved, not applied yet |
| `lane_dimming` | boolean | Saved, not applied yet |
| `ascii` | boolean | Draw with ASCII characters only |
| `reply_preview_limit` | 1, 3, 5, 10, 15, 20, 25 | Replies shown under a card |

A missing field falls back to its default. A field with the wrong type or a value that is not allowed also falls back, and is reported in a toast.

Changing a value on the Appearance tab of Settings (a guest can too) rewrites `config.toml` at once:

- The file is read again just before it is written. Only the five keys above are updated; keys that lazydn does not know are kept, but **comments and formatting are lost**.
- A missing file (and, if needed, its directory and parents, mode 700) is created. The file is written to a temporary file with mode 600 and renamed over `config.toml`, so an existing file ends up with mode 600 too.
- A `config.toml` that is a symlink (for example one managed by home-manager or stow), cannot be read or is not valid TOML is **left alone**: the new value stays for this session and a toast tells you that it was not saved. If the config directory itself is a symlink, the file is written through it.

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

The last visit is saved when you quit with `Q` or `Ctrl+C`; SIGTERM and SIGHUP (closing the terminal) do not save it.

Drafts are saved about two seconds after you stop typing, and at once when you close the composer with `Esc`, post a commit or quit.

### Environment

| Variable | Meaning |
| --- | --- |
| `LAZYDN_CONFIG_DIR` | Config directory, holding the device key. |
| `LAZYDN_STATE_DIR` | State directory. |
| `LAZYDN_BASE_URL` | API host instead of `https://dev-null.sh`. Only `https`, or `http` to localhost, 127.0.0.1 (any 127.0.0.0/8 address) or ::1, is accepted; anything else, including a URL with a user name, a path, a query, a fragment or surrounding spaces, stops lazydn at startup. An empty value counts as unset. The status line shows the host when it is not the default. |
| `COLORTERM` | `truecolor` or `24bit` turns on 24-bit colors. |

## Known limitations

- `theme`, `collapse_nav` and `lane_dimming` are saved but do not change the display yet; the Appearance tab says `(not applied yet)`.
- The Safety tab lists only the first page (20 people) of the muted and the blocked users, so the 21st and later cannot be seen or released from lazydn.
- On macOS the device name is always "lazydn", because the hostname is read from `/proc`.

## Development

```sh
nix develop   # optional: Rust toolchain, just and cargo-insta
just check    # formatting, clippy and the tests
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow, and [docs/](docs/) for the screen design and the dev-null.sh API notes.

The automated tests never connect to dev-null.sh: they use local mock servers and temporary directories, and start no browser.

## License

[MIT](LICENSE)
