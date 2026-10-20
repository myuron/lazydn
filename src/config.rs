//! Where lazydn keeps its files and which host it talks to, decided from the environment main read.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use url::Url;

use crate::api::client::PRODUCTION_BASE_URL;
use crate::app::{LogTab, Prefs, Theme};
use crate::runtime::external::is_loopback;

/// Every environment value lazydn reads. Only `Env::from_process` touches the process environment,
/// so that the rest can be tested with plain values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    /// `LAZYDN_CONFIG_DIR`
    pub config_dir: Option<PathBuf>,
    /// `LAZYDN_STATE_DIR`
    pub state_dir: Option<PathBuf>,
    /// `XDG_CONFIG_HOME`
    pub xdg_config_home: Option<PathBuf>,
    /// `XDG_STATE_HOME`
    pub xdg_state_home: Option<PathBuf>,
    /// `HOME`
    pub home: Option<PathBuf>,
    /// `LAZYDN_BASE_URL`
    pub base_url: Option<String>,
    /// `COLORTERM`
    pub colorterm: Option<String>,
}

impl Env {
    /// Reads the variables once at startup.
    pub fn from_process() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var_os(name))
    }

    /// Builds the environment from any source of variables; an empty value counts as unset.
    /// The two directory overrides must be absolute.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Result<Self, ConfigError> {
        let get = |name: &str| lookup(name).filter(|v| !v.is_empty());
        let path = |name: &str| get(name).map(PathBuf::from);
        let text = |name: &str| get(name).and_then(|v| v.into_string().ok());
        let url = match get("LAZYDN_BASE_URL").map(OsString::into_string) {
            None => None,
            Some(Ok(url)) => Some(url),
            Some(Err(raw)) => {
                return Err(ConfigError::InvalidBaseUrl(
                    raw.to_string_lossy().into_owned(),
                ));
            }
        };
        let absolute = |name: &str| match path(name) {
            Some(p) if !p.is_absolute() => Err(ConfigError::RelativeDir(name.to_string())),
            other => Ok(other),
        };
        Ok(Self {
            config_dir: absolute("LAZYDN_CONFIG_DIR")?,
            state_dir: absolute("LAZYDN_STATE_DIR")?,
            xdg_config_home: path("XDG_CONFIG_HOME"),
            xdg_state_home: path("XDG_STATE_HOME"),
            home: path("HOME"),
            base_url: url,
            colorterm: text("COLORTERM"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// Neither an override nor `XDG_*` nor `HOME` says where to put files.
    NoHome,
    /// `LAZYDN_BASE_URL` is not https and not plain http to a loopback host; carries the offending value.
    InvalidBaseUrl(String),
    /// An override that must be absolute is relative; carries the variable name.
    RelativeDir(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NoHome => write!(
                f,
                "cannot tell where to keep files: set HOME, XDG_CONFIG_HOME or LAZYDN_CONFIG_DIR"
            ),
            ConfigError::InvalidBaseUrl(value) => write!(
                f,
                "LAZYDN_BASE_URL {value:?} must be https, or http to localhost, 127.0.0.1 or ::1, with no path, query or fragment"
            ),
            ConfigError::RelativeDir(name) => write!(f, "{name} must be an absolute path"),
        }
    }
}

impl std::error::Error for ConfigError {}

fn dir_under(
    over: &Option<PathBuf>,
    xdg: &Option<PathBuf>,
    home: &Option<PathBuf>,
    home_suffix: &str,
) -> Result<PathBuf, ConfigError> {
    if let Some(dir) = over {
        return Ok(dir.clone());
    }
    if let Some(base) = xdg.as_ref().filter(|p| p.is_absolute()) {
        return Ok(base.join("lazydn"));
    }
    home.as_ref()
        .filter(|h| h.is_absolute())
        .map(|h| h.join(home_suffix).join("lazydn"))
        .ok_or(ConfigError::NoHome)
}

/// `LAZYDN_CONFIG_DIR`, else an absolute `$XDG_CONFIG_HOME/lazydn`, else `$HOME/.config/lazydn`.
pub fn config_dir(env: &Env) -> Result<PathBuf, ConfigError> {
    dir_under(&env.config_dir, &env.xdg_config_home, &env.home, ".config")
}

/// `LAZYDN_STATE_DIR`, else an absolute `$XDG_STATE_HOME/lazydn`, else `$HOME/.local/state/lazydn`.
pub fn state_dir(env: &Env) -> Result<PathBuf, ConfigError> {
    dir_under(
        &env.state_dir,
        &env.xdg_state_home,
        &env.home,
        ".local/state",
    )
}

/// The device key sits next to `config.toml`.
pub fn key_path(config_dir: &Path) -> PathBuf {
    config_dir.join("id_ed25519")
}

/// A validated API host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    pub url: String,
    /// `host[:port]`, set only when the URL is not the production one, for the status line.
    pub host_label: Option<String>,
}

/// The production host when unset; otherwise the value, if it is https or http to a loopback host.
pub fn base_url(env: &Env) -> Result<BaseUrl, ConfigError> {
    let Some(raw) = env.base_url.as_deref() else {
        return Ok(BaseUrl {
            url: PRODUCTION_BASE_URL.into(),
            host_label: None,
        });
    };
    let invalid = || ConfigError::InvalidBaseUrl(raw.to_string());
    if raw != raw.trim() {
        return Err(invalid());
    }
    let url = Url::parse(raw).map_err(|_| invalid())?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => is_loopback(&url),
        _ => false,
    };
    let bare_root = url.path() == "/" && url.query().is_none() && url.fragment().is_none();
    if !secure || !bare_root || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    let host = url.host_str().ok_or_else(invalid)?;
    let production = Url::parse(PRODUCTION_BASE_URL).map_err(|_| invalid())?;
    let label = (url.origin() != production.origin()).then(|| match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    });
    Ok(BaseUrl {
        url: raw.to_string(),
        host_label: label,
    })
}

/// Whether `COLORTERM` promises 24-bit color.
pub fn is_truecolor(colorterm: Option<&str>) -> bool {
    matches!(colorterm, Some("truecolor" | "24bit"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub prefs: Prefs,
    /// Set when `config.toml` could not be parsed, or a field has the wrong type or a value that is not allowed.
    pub warning: Option<String>,
}

/// Reads `<config_dir>/config.toml`. A missing file gives the defaults and creates nothing;
/// each field that is absent, mistyped or out of range falls back alone.
pub fn load_prefs(config_dir: &Path) -> LoadedConfig {
    let mut loaded = LoadedConfig {
        prefs: Prefs::default(),
        warning: None,
    };
    let text = match std::fs::read_to_string(config_dir.join("config.toml")) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return loaded,
        Err(e) => {
            loaded.warning = Some(format!("config.toml could not be read ({})", e.kind()));
            return loaded;
        }
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        loaded.warning = Some("config.toml could not be parsed; using the defaults".into());
        return loaded;
    };
    let mut bad: Vec<&str> = Vec::new();
    let prefs = &mut loaded.prefs;
    let mut flag = |key: &'static str, slot: &mut bool| match table.get(key) {
        None => {}
        Some(toml::Value::Boolean(b)) => *slot = *b,
        Some(_) => bad.push(key),
    };
    flag("collapse_nav", &mut prefs.collapse_nav);
    flag("lane_dimming", &mut prefs.lane_dimming);
    flag("ascii", &mut prefs.ascii);
    match table.get("theme") {
        None => {}
        Some(toml::Value::String(s)) if s == "dark" => prefs.theme = Theme::Dark,
        Some(toml::Value::String(s)) if s == "light" => prefs.theme = Theme::Light,
        Some(_) => bad.push("theme"),
    }
    match table.get("reply_preview_limit") {
        None => {}
        Some(toml::Value::Integer(n))
            if usize::try_from(*n).is_ok_and(|n| crate::settings::REPLY_LIMITS.contains(&n)) =>
        {
            prefs.reply_preview_limit = *n as usize;
        }
        Some(_) => bad.push("reply_preview_limit"),
    }
    if !bad.is_empty() {
        loaded.warning = Some(format!(
            "config.toml: using the default for {}",
            bad.join(", ")
        ));
    }
    loaded
}

/// The id at the top of a tab when the last session ended.
pub type LastVisits = HashMap<LogTab, String>;

const VISIT_KEYS: [(LogTab, &str); 2] =
    [(LogTab::Following, "following"), (LogTab::Latest, "latest")];

/// Reads `<state_dir>/state.toml`; a missing or corrupt file gives an empty map.
pub fn load_last_visits(state_dir: &Path) -> LastVisits {
    let mut visits = LastVisits::new();
    let Ok(text) = std::fs::read_to_string(state_dir.join("state.toml")) else {
        return visits;
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return visits;
    };
    let Some(section) = table.get("last_visit").and_then(toml::Value::as_table) else {
        return visits;
    };
    for (tab, key) in VISIT_KEYS {
        if let Some(id) = section.get(key).and_then(toml::Value::as_str) {
            visits.insert(tab, id.to_string());
        }
    }
    visits
}

/// Writes `loaded` over the stored values of the same tabs and keeps the other tabs as they were.
/// The file is replaced atomically, and the directory is created when missing.
pub fn save_last_visits(state_dir: &Path, loaded: &LastVisits) -> io::Result<()> {
    if loaded.is_empty() {
        return Ok(());
    }
    let mut merged = load_last_visits(state_dir);
    merged.extend(loaded.iter().map(|(t, id)| (*t, id.clone())));
    let mut section = toml::Table::new();
    for (tab, key) in VISIT_KEYS {
        if let Some(id) = merged.get(&tab) {
            section.insert(key.into(), toml::Value::String(id.clone()));
        }
    }
    let mut root = toml::Table::new();
    root.insert("last_visit".into(), toml::Value::Table(section));
    let text = toml::to_string(&root).map_err(io::Error::other)?;

    std::fs::create_dir_all(state_dir)?;
    let tmp = state_dir.join(format!(".state.toml.tmp-{}", std::process::id()));
    // Synced first so that a crash after the rename cannot leave an empty state.toml.
    let written = write_synced(&tmp, &text)
        .and_then(|()| std::fs::rename(&tmp, state_dir.join("state.toml")));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

fn write_synced(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

const PARSE_TEXT: &str = "config.toml could not be parsed; settings not saved";
const READ_TEXT: &str = "config.toml could not be read; settings not saved";
const SYMLINK_TEXT: &str = "config.toml is a symlink; settings not saved";

fn write_error(e: impl std::fmt::Display) -> String {
    format!("settings could not be saved ({e})")
}

/// Writes the Appearance values into `<config_dir>/config.toml`; the error is the text for the toast.
///
/// The file is read again first and only the five known keys are replaced, so that keys of a newer version survive.
/// A symlinked, unreadable or unparsable file is left alone. Why not write through a symlink: the file may belong
/// to a tool such as home-manager, which a plain file would silently replace.
pub fn save_prefs(config_dir: &Path, prefs: &Prefs) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    let path = config_dir.join("config.toml");
    let mut table = toml::Table::new();
    let mut make_dir = false;
    match std::fs::metadata(config_dir) {
        Ok(m) if m.is_dir() => {
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.file_type().is_symlink() => return Err(SYMLINK_TEXT.into()),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(READ_TEXT.into()),
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    table = text
                        .parse::<toml::Table>()
                        .map_err(|_| PARSE_TEXT.to_string())?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(READ_TEXT.into()),
            }
        }
        Ok(_) => return Err(write_error("not a directory")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => make_dir = true,
        Err(e) => return Err(write_error(e)),
    }

    table.insert(
        "theme".into(),
        toml::Value::String(
            match prefs.theme {
                Theme::Dark => "dark",
                Theme::Light => "light",
            }
            .into(),
        ),
    );
    table.insert(
        "collapse_nav".into(),
        toml::Value::Boolean(prefs.collapse_nav),
    );
    table.insert(
        "lane_dimming".into(),
        toml::Value::Boolean(prefs.lane_dimming),
    );
    table.insert(
        "reply_preview_limit".into(),
        toml::Value::Integer(prefs.reply_preview_limit as i64),
    );
    table.insert("ascii".into(), toml::Value::Boolean(prefs.ascii));
    let text = toml::to_string(&table).map_err(write_error)?;

    if make_dir {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(config_dir)
            .map_err(write_error)?;
    }
    let pid = std::process::id();
    let mut n = 0u32;
    let (tmp, mut file) = loop {
        let tmp = config_dir.join(format!(".config.toml.tmp-{pid}-{n}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(e) => return Err(write_error(e)),
        }
    };
    use std::io::Write;
    // Synced first so that a crash after the rename cannot leave an empty config.toml.
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(write_error(e));
    }
    Ok(())
}
