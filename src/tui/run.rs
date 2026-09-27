//! Terminal runtime (§8.2): owns the terminal, the event loop and the backend.
//!
//! The UI thread runs a plain synchronous loop — poll input, drain backend
//! messages, tick, draw. Everything slow lives on a background tokio runtime
//! and comes back through an mpsc channel.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::app::{App, Effect, GKey, Msg, Opts, Prefs, Screen, UiState};
use super::exec::{self, Backend, Demo, Live};
use super::theme::{self, Theme, ThemeName};
use super::{demo, ui, update};
use crate::config::Config;

/// `lox tui` flags.
#[derive(Debug, Default, Clone)]
pub struct TuiArgs {
    pub screen: Option<String>,
    pub room: Option<String>,
    pub read_only: bool,
    pub theme: Option<String>,
    pub icons: Option<String>,
    pub no_mouse: bool,
    pub no_motion: bool,
    pub no_color: bool,
    pub demo: bool,
}

const TICK: Duration = Duration::from_millis(100);
const FRAME: Duration = Duration::from_millis(33);

static TERMINATE: AtomicBool = AtomicBool::new(false);
static KBD_ENHANCED: AtomicBool = AtomicBool::new(false);
static MOUSE: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    TERMINATE.store(true, Ordering::SeqCst);
}

/// The controlling terminal went away (window closed, tmux session killed).
/// crossterm's `read()` then spins forever on a readable-but-empty fd, so the
/// loop has to notice this itself before reading.
#[cfg(unix)]
fn tty_hung_up() -> bool {
    let mut p = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let n = unsafe { libc::poll(&mut p, 1, 0) };
    // Not POLLNVAL: macOS poll(2) doesn't support tty devices and reports
    // POLLNVAL for a perfectly healthy terminal.
    n > 0 && p.revents & (libc::POLLHUP | libc::POLLERR) != 0
}

#[cfg(not(unix))]
fn tty_hung_up() -> bool {
    false
}

/// Last line of defense: a signal or a hang-up that doesn't end the event loop
/// within 2 s (stuck in a blocking call) ends the process, so a closed
/// terminal can never leave a TUI spinning in the background.
fn spawn_watchdog() {
    let _ = std::thread::Builder::new()
        .name("lox-tui-watchdog".into())
        .spawn(|| {
            let mut since: Option<Instant> = None;
            loop {
                std::thread::sleep(Duration::from_millis(250));
                if tty_hung_up() {
                    TERMINATE.store(true, Ordering::SeqCst);
                }
                if TERMINATE.load(Ordering::SeqCst) {
                    let t = *since.get_or_insert_with(Instant::now);
                    if t.elapsed() > Duration::from_secs(2) {
                        std::process::exit(1);
                    }
                }
            }
        });
}

fn prefs_path() -> PathBuf {
    Config::dir().join("tui.yaml")
}

pub fn load_prefs() -> Prefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|s| serde_yaml::from_str(&s).ok())
        .unwrap_or_default()
}

fn state_path(data_dir: Option<&PathBuf>) -> Option<PathBuf> {
    data_dir.map(|d| d.join("tui-state.yaml"))
}

fn load_state(data_dir: Option<&PathBuf>) -> UiState {
    state_path(data_dir)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_yaml::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(data_dir: Option<&PathBuf>, st: &UiState) {
    let Some(p) = state_path(data_dir) else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_yaml::to_string(st) {
        // write-then-rename: never leave a half-written file behind
        let tmp = p.with_extension("yaml.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

// ── Terminal setup / teardown ───────────────────────────────────────────────

fn init_terminal(mouse: bool) -> Result<()> {
    enable_raw_mode().context("terminal does not support raw mode")?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableBracketedPaste,
        cursor::Hide
    )?;
    if mouse {
        execute!(out, EnableMouseCapture)?;
        MOUSE.store(true, Ordering::SeqCst);
    }
    // Disambiguated keys (Esc vs Alt, Ctrl-i vs Tab) where the terminal supports it
    if matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    ) {
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        KBD_ENHANCED.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// Best effort, idempotent: used on exit, suspend, errors and from the panic hook.
fn restore_terminal() {
    let mut out = io::stdout();
    if KBD_ENHANCED.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    if MOUSE.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, DisableMouseCapture);
    }
    let _ = execute!(
        out,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        cursor::Show
    );
    let _ = disable_raw_mode();
}

struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn osc52(text: &str) {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut out = io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", b64);
    let _ = out.flush();
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub fn run(args: TuiArgs) -> Result<()> {
    use std::io::IsTerminal;
    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        bail!("lox tui needs an interactive terminal (stdin and stdout must be a TTY)");
    }
    // -v request logging goes to stderr, which would draw over the screen
    // (and print URL paths such as an alarm PIN)
    crate::client::set_verbose(0);
    let prefs = load_prefs();

    // theme: flag → --no-color → tui.yaml → NO_COLOR → detection (§6.1)
    let flag_theme = match &args.theme {
        Some(t) => Some(
            ThemeName::parse(t)
                .with_context(|| format!("unknown theme '{}' (night, day, neon, mono)", t))?,
        ),
        None => None,
    };
    let cfg_theme = prefs.theme.as_deref().and_then(ThemeName::parse);
    let choice = theme::resolve(flag_theme, args.no_color, cfg_theme, &|k| {
        std::env::var(k).ok()
    });
    let th = Theme::new(choice.theme, choice.depth);

    let icons = args
        .icons
        .clone()
        .or(prefs.icons.clone())
        .unwrap_or_else(|| "plain".into());
    if icons != "plain" && icons != "nerd" {
        bail!("--icons must be 'plain' or 'nerd'");
    }
    let opts = Opts {
        read_only: args.read_only,
        demo: args.demo,
        mouse: !args.no_mouse && prefs.mouse.unwrap_or(true),
        motion: !args.no_motion
            && prefs.motion.unwrap_or(true)
            && std::env::var("LOX_NO_MOTION").is_err(),
        nerd: icons == "nerd",
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("lox-tui")
        .build()?;
    let (tx, rx) = mpsc::channel::<Msg>();

    // Load everything that can fail *before* touching the terminal.
    let (house, ctx_name, data_dir, contexts, scenes, mut backend) = if args.demo {
        let st = demo::structure();
        let house = exec::build_house(&st, &prefs.roles);
        let b = Backend::Demo(Box::new(Demo::new(rt.handle().clone(), tx.clone(), st)));
        let scenes = exec::DEMO_SCENES.iter().map(|s| s.to_string()).collect();
        (
            house,
            "demo".to_string(),
            None,
            vec!["demo".into(), "office".into(), "cabin".into()],
            scenes,
            b,
        )
    } else {
        let cfg = Config::load()?;
        if cfg.host.is_empty() {
            bail!(
                "no Miniserver configured — run `lox setup set --host … --user … --pass …` or try `lox tui --demo`"
            );
        }
        eprint!("loading structure… ");
        let st = Live::load_structure(&cfg).context("could not load the structure file")?;
        eprintln!("ok");
        let house = exec::build_house(&st, &prefs.roles);
        let name = cfg.context_name.clone().unwrap_or_else(|| "default".into());
        let mut contexts = exec::context_names();
        if !contexts.contains(&name) {
            contexts.insert(0, name.clone());
        }
        let scenes = crate::scene::Scene::list_with_config(&cfg).unwrap_or_default();
        let data_dir = Some(cfg.data_dir.clone());
        let b = Backend::Live(Box::new(Live::new(
            rt.handle().clone(),
            tx.clone(),
            cfg,
            prefs.roles.clone(),
        )?));
        (house, name, data_dir, contexts, scenes, b)
    };

    let mut app = App::new(house, th, opts, ctx_name, exec::now());
    app.tz = exec::tz_offset();
    app.contexts = contexts;
    app.scenes = scenes;
    let st = load_state(data_dir.as_ref());
    let last = st.last_screen;
    app.load_ui_state(st);
    let mut data_dir = data_dir;

    // start screen: --screen, then -r, then where you left off
    if let Some(s) = &args.screen {
        let s = Screen::parse(s).with_context(|| {
            format!(
                "unknown screen '{}' (home, rooms, events, energy, system, sites)",
                s
            )
        })?;
        update::go(&mut app, s);
    } else if let Some(s) = last {
        update::go(&mut app, s);
    }
    if let Some(r) = &args.room {
        let q = r.to_lowercase();
        let rooms = &app.house.rooms;
        let idx = rooms
            .iter()
            .position(|x| x.name.to_lowercase() == q)
            .or_else(|| {
                rooms
                    .iter()
                    .position(|x| x.name.to_lowercase().contains(&q))
            })
            .with_context(|| format!("no room matches '{}'", r))?;
        update::go(&mut app, Screen::Rooms);
        app.rooms.sel_group = Some(GKey::Room(idx));
        app.rooms.pane = 1;
    }

    // signals: SIGTERM/SIGHUP end the loop cleanly (terminal restored, state saved)
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        #[cfg(unix)]
        libc::signal(libc::SIGHUP, on_signal as *const () as libc::sighandler_t);
    }
    spawn_watchdog();
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        // a panic on a backend thread leaves the UI thread running: end it
        // rather than draw over the restored screen
        TERMINATE.store(true, Ordering::SeqCst);
        prev_hook(info);
    }));

    init_terminal(app.opts.mouse)?;
    let guard = TermGuard;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let sz = terminal.size()?;
    update::update(&mut app, Msg::Resize(sz.width, sz.height));

    backend.start(&app);
    let initial = update::schedule(&mut app);
    for e in initial {
        backend.handle(e, &app);
    }

    let result = event_loop(&mut app, &mut terminal, &mut backend, &rx, &mut data_dir);

    save_state(data_dir.as_ref(), &update::ui_state(&app));
    backend.stop();
    drop(terminal);
    drop(guard);
    drop(backend);
    rt.shutdown_timeout(Duration::from_millis(300));
    result
}

type Term = Terminal<CrosstermBackend<io::Stdout>>;

fn event_loop(
    app: &mut App,
    terminal: &mut Term,
    backend: &mut Backend,
    rx: &mpsc::Receiver<Msg>,
    data_dir: &mut Option<PathBuf>,
) -> Result<()> {
    let mut last_tick = Instant::now() - TICK;
    let mut last_draw = Instant::now() - FRAME;
    let mut dirty = true;
    loop {
        if TERMINATE.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut effects = Vec::new();

        // 1. backend messages (bounded per frame so input stays responsive)
        for _ in 0..500 {
            let Ok(msg) = rx.try_recv() else { break };
            // context switch: persist the old context's state, load the new one's
            let switch = matches!(&msg, Msg::NewHouse { ctx, .. } if *ctx != app.ctx_name);
            if switch {
                save_state(data_dir.as_ref(), &update::ui_state(app));
            }
            let new_ctx = if let Msg::NewHouse { ctx, .. } = &msg {
                Some(ctx.clone())
            } else {
                None
            };
            effects.extend(update::update(app, msg));
            if let (true, Some(ctx)) = (switch, new_ctx) {
                if let Some(cfg) = backend.cfg() {
                    *data_dir = Some(cfg.data_dir.clone());
                    app.scenes = crate::scene::Scene::list_with_config(cfg).unwrap_or_default();
                }
                let st = load_state(data_dir.as_ref());
                app.load_ui_state(st);
                app.toast(super::app::ToastKind::Ok, format!("✓ {}", ctx));
            }
            dirty = true;
        }

        // 2. input
        let until_tick = TICK.saturating_sub(last_tick.elapsed());
        let timeout = if dirty {
            until_tick.min(FRAME.saturating_sub(last_draw.elapsed()))
        } else {
            until_tick
        };
        let ready = event::poll(timeout)?;
        if tty_hung_up() {
            return Ok(());
        }
        if ready {
            // drain everything that is already queued
            loop {
                let msg = match event::read()? {
                    Event::Key(k) => Some(Msg::Key(k)),
                    Event::Paste(s) => Some(Msg::Paste(s)),
                    Event::Mouse(m) if app.opts.mouse => Some(Msg::Mouse(m)),
                    Event::Resize(w, h) => Some(Msg::Resize(w, h)),
                    _ => None,
                };
                if let Some(m) = msg {
                    effects.extend(update::update(app, m));
                    dirty = true;
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }

        // 3. tick
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            effects.extend(update::update(app, Msg::Tick { now: exec::now() }));
            dirty = true;
        }

        // 4. effects
        for e in update::coalesce(effects) {
            match e {
                Effect::Quit => return Ok(()),
                Effect::Copy(s) => osc52(&s),
                Effect::SaveState(st) => save_state(data_dir.as_ref(), &st),
                // Ctrl-Z: job control exists on Unix only
                #[cfg(unix)]
                Effect::Suspend => {
                    save_state(data_dir.as_ref(), &update::ui_state(app));
                    restore_terminal();
                    unsafe {
                        libc::raise(libc::SIGTSTP);
                    }
                    // resumed (SIGCONT)
                    init_terminal(app.opts.mouse)?;
                    terminal.clear()?;
                    dirty = true;
                }
                #[cfg(not(unix))]
                Effect::Suspend => {}
                other => backend.handle(other, app),
            }
        }
        if app.quit {
            return Ok(());
        }

        // 5. draw (≤ 30 fps)
        if dirty && last_draw.elapsed() >= FRAME {
            draw(terminal, app)?;
            last_draw = Instant::now();
            dirty = false;
        }
    }
}

fn draw(terminal: &mut Term, app: &App) -> Result<()> {
    let _ = execute!(io::stdout(), BeginSynchronizedUpdate);
    let r = terminal.draw(|f| {
        let area = f.area();
        ui::render(app, area, f.buffer_mut());
    });
    let _ = execute!(io::stdout(), EndSynchronizedUpdate);
    r?;
    Ok(())
}
