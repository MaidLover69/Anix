//! "Alerts while Anix is closed": `anix --background` runs only the release notifier and the
//! release checks, with no window and no webview. It is started at login, by an autostart
//! entry or a systemd user timer (see [`install`]).
//!
//! Exactly one process checks at a time, settled with three lock files in the data folder:
//! - the app holds [`APP_LOCK`] for as long as it runs;
//! - whoever checks holds [`CHECKER_LOCK`]; the app waits for it after taking its own lock;
//! - a background process holds [`BACKGROUND_LOCK`], so a second one exits at once.
//!
//! Before every step the background process looks at the app's lock. While the app runs, it
//! lets go of the checker lock and stands by, and the app takes over; when the app quits, the
//! background process takes the checker lock again. With no bells it also lets go of the
//! checker lock between checks (then 30 seconds apart, reading its state again each
//! time), so an app started meanwhile takes over at once. Hits and
//! final notes are also recorded with updates that succeed only once, so even an overlap never
//! alerts twice.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const APP_LOCK: &str = "app.lock";
pub const CHECKER_LOCK: &str = "release-check.lock";
pub const BACKGROUND_LOCK: &str = "background.lock";

/// An exclusive `flock` on a file, released when dropped (or when the process ends).
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

/// Takes the lock on `path` if nobody holds it.
pub fn try_lock(path: &Path) -> io::Result<Option<Lock>> {
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    // SAFETY: flock on a file descriptor this function owns.
    let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if taken {
        return Ok(Some(Lock { _file: file }));
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(err)
    }
}

/// Whether some process (this one included, through another open file) holds `path`.
pub fn is_held(path: &Path) -> bool {
    matches!(try_lock(path), Ok(None))
}

/// The checker's side of the hand-off, kept between steps of a background process.
#[derive(Debug)]
pub struct Handoff {
    dir: PathBuf,
    checker: Option<Lock>,
}

impl Handoff {
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf(), checker: None }
    }

    /// Whether this process may check now: not while the app runs, and only with the checker
    /// lock, which is let go of while standing by.
    pub fn may_check(&mut self) -> bool {
        if is_held(&self.dir.join(APP_LOCK)) {
            self.checker = None;
            return false;
        }
        if self.checker.is_none() {
            self.checker = try_lock(&self.dir.join(CHECKER_LOCK)).ok().flatten();
        }
        self.checker.is_some()
    }

    pub fn holds_checker(&self) -> bool {
        self.checker.is_some()
    }

    /// No bells: the app must not wait for the next step, so the lock is let go of until
    /// [`Handoff::may_check`] takes it again. True when it was held.
    pub fn idle(&mut self) -> bool {
        self.checker.take().is_some()
    }
}

/// The background process's gate. Its `may_check` also reads the settings file, so tests use
/// the inherent [`Handoff`] methods (which take precedence) on temp folders.
impl crate::notifier::CheckGate for Handoff {
    fn may_check(&mut self) -> bool {
        // Switched off in Settings while running: nothing should check from here on.
        if !super::SystemSettings::read().background_alerts {
            tracing::info!("Alerts while Anix is closed are switched off; the background process exits");
            std::process::exit(0);
        }
        Handoff::may_check(self)
    }

    fn idle(&mut self) -> bool {
        Handoff::idle(self)
    }
}

/// The app's side: its own lock, then the checker lock once a background process has stood
/// by. The locks are kept until the app quits. A lock that cannot be used at all (an
/// unwritable folder) is skipped rather than holding up the notifier.
pub async fn claim_in(dir: &Path, poll: Duration) -> Vec<Lock> {
    let mut held = Vec::new();
    // A background process takes the app's lock for a moment before every step.
    for (name, wait) in [(APP_LOCK, Duration::from_millis(100)), (CHECKER_LOCK, poll)] {
        let mut waited = false;
        loop {
            match try_lock(&dir.join(name)) {
                Ok(Some(lock)) => {
                    if waited && name == CHECKER_LOCK {
                        tracing::info!("Release checks moved from the background process to the app");
                    }
                    held.push(lock);
                    break;
                }
                Ok(None) => {
                    waited = true;
                    tokio::time::sleep(wait).await;
                }
                Err(e) => {
                    tracing::warn!("Release checks run without the {} lock: {}", name, e);
                    break;
                }
            }
        }
    }
    held
}

/// [`claim_in`] for the app's data folder.
pub async fn claim_for_app() -> Vec<Lock> {
    match crate::db::get_app_dir() {
        Ok(dir) => claim_in(&dir, Duration::from_secs(1)).await,
        Err(_) => Vec::new(),
    }
}

/// Entry point of `--background`; returns the process exit code.
pub fn run() -> i32 {
    let bases = crate::migration::BaseDirs::system();
    let migrated = crate::migration::migrate(&bases);
    crate::init_logging();
    migrated.log();

    let dir = match crate::db::get_app_dir() {
        Ok(dir) => dir,
        Err(e) => {
            tracing::error!("Background release checks cannot start: {}", e);
            return 1;
        }
    };
    let _single = match try_lock(&dir.join(BACKGROUND_LOCK)) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            tracing::info!("Background release checks are already running");
            return 0;
        }
        Err(e) => {
            tracing::error!("Background release checks cannot start: {}", e);
            return 1;
        }
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::error!("Background release checks cannot start: {}", e);
            return 1;
        }
    };
    runtime.block_on(async move {
        let pool = match crate::db::init_db().await {
            Ok(pool) => pool,
            Err(e) => {
                tracing::error!("Background release checks cannot open the database: {}", e);
                return 1;
            }
        };
        let state = crate::state::AppState::new(pool);
        state.plugins.load_all().await;
        tracing::info!("Background release checks started (no window)");

        let on_click: crate::notifier::OnClick = Arc::new(open_app);
        let sink = crate::notifier::DesktopSink::new(
            "Anix".to_string(),
            state.client.clone(),
            crate::notifier::cover_dir(),
            on_click,
        );
        crate::notifier::drive(
            &state.pool,
            &*state.catalogue,
            crate::notifier::extension_probe(state.plugins.clone()),
            &sink,
            Handoff::new(&dir),
        )
        .await;
        0
    })
}

/// The arguments the app is started with for a click on a background alert, or `None` for no
/// start. An anime goes as its `anix://` link: a fresh app opens it at start, and a running one
/// gets it from the single-instance hand-off, both through the deep-link plugin.
pub fn launch_args(click: crate::notifier::Click) -> Option<Vec<String>> {
    use crate::notifier::Click;
    match click {
        Click::Nothing => None,
        Click::Focus => Some(Vec::new()),
        Click::OpenAnime(id) => Some(vec![format!("anix://anime/{}", id)]),
    }
}

/// A click on an alert from the background process starts the app (or brings it forward) on
/// the alert's anime.
fn open_app(click: crate::notifier::Click) {
    let Some(args) = launch_args(click) else {
        return;
    };
    let Some(exec) = super::autostart::current_exec_target() else {
        return;
    };
    let mut cmd = std::process::Command::new(&exec);
    cmd.args(args);
    if let Err(e) = spawn_reaped(cmd) {
        tracing::warn!("Could not start Anix from an alert: {}", e);
    }
}

/// Starts `cmd` and waits for it on a thread of its own, so it leaves no zombie behind.
fn spawn_reaped(mut cmd: std::process::Command) -> io::Result<()> {
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Starting at login
// ---------------------------------------------------------------------------------------

pub const AUTOSTART_FILE_NAME: &str = "anix-release-check.desktop";
pub const UNIT_NAME: &str = "anix-release-check";
/// Marks files this app wrote, so a hand-made file of the same name is never touched.
const MARKER: &str = "X-Anix-Background=true";

/// How the background process is started at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Starter {
    /// `~/.config/systemd/user/anix-release-check.{service,timer}`.
    SystemdTimer,
    /// `~/.config/autostart/anix-release-check.desktop`.
    Autostart,
}

/// A systemd user session is used when there is one, else the XDG autostart entry.
pub fn starter(systemd_user_session: bool) -> Starter {
    if systemd_user_session {
        Starter::SystemdTimer
    } else {
        Starter::Autostart
    }
}

/// Whether this login has a systemd user manager.
pub fn systemd_user_session() -> bool {
    std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|dir| Path::new(&dir).join("systemd").is_dir())
}

pub fn autostart_path(config_home: &Path) -> PathBuf {
    config_home.join("autostart").join(AUTOSTART_FILE_NAME)
}

pub fn unit_paths(config_home: &Path) -> (PathBuf, PathBuf) {
    let dir = config_home.join("systemd").join("user");
    (dir.join(format!("{}.service", UNIT_NAME)), dir.join(format!("{}.timer", UNIT_NAME)))
}

pub fn autostart_entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Anix release alerts\n\
         Comment=Checks for new episodes while Anix is closed\n\
         Exec={} {}\n\
         Icon=anix\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n\
         {}\n",
        super::autostart::quote_exec_arg(&exec.to_string_lossy()),
        super::BACKGROUND_ARG,
        MARKER
    )
}

/// One word of a systemd `ExecStart=`: quoted when it has spaces, quotes or backslashes, with
/// `%` and `$` doubled so systemd expands neither.
pub fn quote_systemd_arg(arg: &str) -> String {
    let escaped = arg.replace('%', "%%").replace('$', "$$");
    if escaped.is_empty() || escaped.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | ';')) {
        format!("\"{}\"", escaped.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        escaped
    }
}

pub fn service_unit(exec: &Path) -> String {
    format!(
        "# {}\n\
         [Unit]\n\
         Description=Anix release alerts while the app is closed\n\
         \n\
         [Service]\n\
         Type=simple\n\
         KillMode=process\n\
         ExecStart={} {}\n",
        MARKER,
        quote_systemd_arg(&exec.to_string_lossy()),
        super::BACKGROUND_ARG
    )
}

/// Starts the service after login, and again a while after it stops.
pub fn timer_unit() -> String {
    format!(
        "# {}\n\
         [Unit]\n\
         Description=Starts Anix release alerts after login\n\
         \n\
         [Timer]\n\
         OnStartupSec=1min\n\
         OnUnitInactiveSec=15min\n\
         Unit={}.service\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        MARKER, UNIT_NAME
    )
}

/// Writes `wanted` to `path` (or removes the file with `None`); only files this app wrote are
/// replaced or removed. Returns whether anything changed.
fn write_ours(path: &Path, wanted: Option<&str>) -> io::Result<bool> {
    let existing = std::fs::read_to_string(path).ok();
    let ours = existing.as_deref().map_or(true, |text| text.contains(MARKER));
    match wanted {
        Some(wanted) => {
            if existing.as_deref() == Some(wanted) {
                return Ok(false);
            }
            if !ours {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and was not made by Anix", path.display()),
                ));
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = path.with_extension(format!("tmp{}", std::process::id()));
            std::fs::write(&tmp, wanted)?;
            std::fs::rename(&tmp, path).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })?;
            Ok(true)
        }
        None if existing.is_some() && ours => {
            std::fs::remove_file(path)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Writes the files that start `exec --background` at login (`exec` given), or removes them.
/// Only the files of `how` are written; the other kind is removed. Returns whether the
/// systemd units changed, in which case systemd has to be told.
pub fn install_files(config_home: &Path, exec: Option<&Path>, how: Starter) -> io::Result<bool> {
    let (service, timer) = unit_paths(config_home);
    let systemd = exec.filter(|_| how == Starter::SystemdTimer);
    let autostart = exec.filter(|_| how == Starter::Autostart);
    write_ours(&autostart_path(config_home), autostart.map(autostart_entry).as_deref())?;
    let mut changed = write_ours(&service, systemd.map(service_unit).as_deref())?;
    changed |= write_ours(&timer, systemd.map(|_| timer_unit()).as_deref())?;
    Ok(changed)
}

/// Whether one of the starters is in place under `config_home`.
pub fn is_installed(config_home: &Path) -> bool {
    let ours = |path: PathBuf| std::fs::read_to_string(path).is_ok_and(|text| text.contains(MARKER));
    ours(autostart_path(config_home)) || ours(unit_paths(config_home).1)
}

fn systemctl(args: &[&str]) {
    match std::process::Command::new("systemctl").arg("--user").args(args).output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => tracing::warn!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => tracing::warn!("systemctl could not be run: {}", e),
    }
}

/// Applies the "Alerts while Anix is closed" switch for this login.
pub fn install(config_home: &Path, exec: Option<&Path>) -> io::Result<()> {
    let how = starter(systemd_user_session());
    let timer = format!("{}.timer", UNIT_NAME);
    if exec.is_none() && how == Starter::SystemdTimer && unit_paths(config_home).1.exists() {
        systemctl(&["disable", "--now", &timer]);
        systemctl(&["stop", &format!("{}.service", UNIT_NAME)]);
    }
    if install_files(config_home, exec, how)? && how == Starter::SystemdTimer {
        systemctl(&["daemon-reload"]);
        if exec.is_some() {
            systemctl(&["enable", "--now", &timer]);
        }
    }
    // Without systemd nothing would check until the next login; a second copy exits at once.
    if let Some(exec) = exec.filter(|_| how == Starter::Autostart) {
        let mut cmd = std::process::Command::new(exec);
        cmd.arg(super::BACKGROUND_ARG);
        if let Err(e) = spawn_reaped(cmd) {
            tracing::warn!("Could not start the background release checks: {}", e);
        }
    }
    Ok(())
}
