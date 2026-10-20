use std::io::stdout;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use color_eyre::Result;
use color_eyre::config::HookBuilder;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::supports_keyboard_enhancement;
use futures_util::{FutureExt, StreamExt};
use lazydn::action::Action;
use lazydn::api::clock::SystemClock;
use lazydn::config::{self, Env};
use lazydn::runtime::exec::TokioExec;
use lazydn::runtime::external::{Osc52Clipboard, SystemLauncher};
use lazydn::runtime::run::{ClientKind, Control, FileStore, Runtime};
use lazydn::runtime::sign_in::device_name;
use lazydn::runtime::startup::{Startup, build_app, session_for_key};
use lazydn::{keymap, ui};
use ratatui::DefaultTerminal;
use time::{OffsetDateTime, UtcOffset};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// Whether the keyboard flags were pushed, so that only then are they popped.
static KEYBOARD_PUSHED: AtomicBool = AtomicBool::new(false);

type AppRuntime = Runtime<TokioExec, SystemLauncher, Osc52Clipboard<std::io::Stdout>, FileStore>;

fn main() -> Result<()> {
    // Must run before any thread exists; the time crate refuses to read the offset afterwards.
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);

    let (panic_hook, eyre_hook) = HookBuilder::default().into_hooks();
    eyre_hook.install()?;

    // Every setting is checked before the terminal is touched, so that an error prints normally.
    let env = Env::from_process()?;
    let config_dir = config::config_dir(&env)?;
    let state_dir = config::state_dir(&env)?;
    let base = config::base_url(&env)?;
    let key_path = config::key_path(&config_dir);
    let (session, key_status, key) = session_for_key(&key_path);
    let mut startup = Startup {
        session,
        key_status,
        key_path: key_path.clone(),
        config: config::load_prefs(&config_dir),
        truecolor: config::is_truecolor(env.colorterm.as_deref()),
        host_label: base.host_label.clone(),
        last_visits: config::load_last_visits(&state_dir),
        now: OffsetDateTime::now_utc(),
        offset: local_offset,
        size: crossterm::terminal::size().unwrap_or((80, 24)),
        keyboard: lazydn::keyboard::KeyboardMode::default(),
    };

    let runtime = tokio::runtime::Runtime::new()?;
    let (tx, rx) = unbounded_channel();
    let exec = {
        let _guard = runtime.enter();
        TokioExec::new(
            &base.url,
            key_path,
            &device_name(),
            key,
            Arc::new(SystemClock),
            tx,
        )?
    };
    // ratatui chains its own restoring hook onto this one; paste goes off first, then the
    // terminal is restored, and only then does color_eyre print the report on a usable screen.
    std::panic::set_hook(panic_hook.into_panic_hook());
    let mut terminal = ratatui::init();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        pop_keyboard_flags();
        let _ = crossterm::execute!(stdout(), DisableBracketedPaste);
        previous(info);
        // Not left running: a panic in a tokio task would otherwise leave the app alive on a
        // restored terminal with no one reading the keys. Exiting here rather than via a
        // `panic = "abort"` profile keeps the behaviour the same in every build profile.
        std::process::exit(101);
    }));
    let _ = crossterm::execute!(stdout(), EnableBracketedPaste);
    // A failed query means a terminal that cannot be asked, which counts as unsupported.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced
        && crossterm::execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok()
    {
        KEYBOARD_PUSHED.store(true, Ordering::SeqCst);
    }
    startup.keyboard = lazydn::keyboard::KeyboardMode {
        enhanced: KEYBOARD_PUSHED.load(Ordering::SeqCst),
    };
    let (app, first_effects) = build_app(startup);
    let app_runtime = Runtime {
        app,
        client: ClientKind::Guest,
        exec,
        launcher: SystemLauncher,
        clipboard: Osc52Clipboard::new(stdout()),
        store: FileStore { dir: state_dir },
        base_url: base.url,
    };

    let result = runtime.block_on(run(app_runtime, &mut terminal, rx, first_effects));
    pop_keyboard_flags();
    let _ = crossterm::execute!(stdout(), DisableBracketedPaste);
    ratatui::restore();
    if let Ok(Control::Quit {
        save_error: Some(error),
    }) = &result
    {
        eprintln!("lazydn: could not save: {error}");
    }
    result.map(|_| ())
}

fn pop_keyboard_flags() {
    if KEYBOARD_PUSHED.swap(false, Ordering::SeqCst) {
        let _ = crossterm::execute!(stdout(), PopKeyboardEnhancementFlags);
    }
}

async fn run(
    mut rt: AppRuntime,
    terminal: &mut DefaultTerminal,
    mut rx: UnboundedReceiver<Action>,
    first_effects: Vec<lazydn::action::Effect>,
) -> Result<Control> {
    let mut control = rt.apply(first_effects);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // A failed draw or event read returns here without saving; the state is only worth keeping
    // from a session that ended on purpose.
    while control == Control::Continue {
        terminal.draw(|frame| ui::draw(&rt.app, frame))?;
        let first = tokio::select! {
            event = events.next() => match event {
                Some(event) => action_of(&rt, event?),
                None => return Ok(rt.handle(Action::Quit)),
            },
            _ = tick.tick() => Some(Action::Tick(OffsetDateTime::now_utc())),
            action = rx.recv() => action,
        };
        control = step(&mut rt, first);
        // Everything that piled up is handled before the one draw above, so a burst costs one frame.
        while control == Control::Continue {
            if let Ok(action) = rx.try_recv() {
                control = rt.handle(action);
                continue;
            }
            match events.next().now_or_never() {
                Some(Some(event)) => {
                    let action = action_of(&rt, event?);
                    control = step(&mut rt, action);
                }
                Some(None) => return Ok(rt.handle(Action::Quit)),
                None => break,
            }
        }
    }
    Ok(control)
}

fn step(rt: &mut AppRuntime, action: Option<Action>) -> Control {
    match action {
        Some(action) => rt.handle(action),
        None => Control::Continue,
    }
}

fn action_of(rt: &AppRuntime, event: Event) -> Option<Action> {
    keymap::map_event(&rt.app, &event)
}
