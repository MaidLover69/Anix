//! Carries an install made under the app's earlier name (Anilili) over to the current one.
//!
//! The earlier name was part of every place the app keeps things in: the folders in the data,
//! config and cache directories, and the folder named after the bundle identifier that holds the
//! webview's storage and the site sessions. `migrate` gives those folders their current names.
//! It runs first thing at startup, before the log file, the database or a webview is opened.

pub use crate::db::LEGACY_APP_DIR_NAME;
use crate::db::{APP_DIR_NAME, DB_FILE_NAME, LEGACY_DB_FILE_NAME};
use sqlx::{Row, SqlitePool};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The bundle identifier in `tauri.conf.json`; Tauri names the webview's data folder after it.
pub const IDENTIFIER: &str = "io.anix.app";
pub const LEGACY_IDENTIFIER: &str = "io.anilili.linux";
pub const LOG_FILE_NAME: &str = "anix.log";
pub const LEGACY_LOG_FILE_NAME: &str = "anilili.log";

/// The directories the app's own folders live in. `None` for one that is unknown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BaseDirs {
    /// `$XDG_DATA_HOME`: the database, downloads, plugins and the log.
    pub data: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`: `settings.json`.
    pub config: Option<PathBuf>,
    /// `$XDG_CACHE_HOME`.
    pub cache: Option<PathBuf>,
    /// Where Tauri puts the folder named after the identifier (the same as `data` on Linux).
    pub local_data: Option<PathBuf>,
    /// The home directory, for the `~/.local/share` and `~/.config` folders an install keeps
    /// using when the XDG variables point elsewhere.
    pub home: Option<PathBuf>,
}

impl BaseDirs {
    /// The directories of the user running the app.
    pub fn system() -> Self {
        Self {
            data: dirs::data_dir(),
            config: dirs::config_dir(),
            cache: dirs::cache_dir(),
            local_data: dirs::data_local_dir(),
            home: dirs::home_dir(),
        }
    }

    /// Every (earlier, current) pair of data folders: the one in the data directory and, when
    /// that is somewhere else, the one in `~/.local/share`.
    pub fn data_dirs(&self) -> Vec<(PathBuf, PathBuf)> {
        let mut pairs = Vec::new();
        let bases = [
            self.data.clone(),
            self.home.as_ref().map(|home| home.join(".local").join("share")),
        ];
        for base in bases.into_iter().flatten() {
            push_pair(&mut pairs, &base, LEGACY_APP_DIR_NAME, APP_DIR_NAME);
        }
        pairs
    }

    /// Every (earlier, current) pair of folders, in the order they are moved.
    fn folders(&self) -> Vec<(PathBuf, PathBuf)> {
        let mut pairs = self.data_dirs();
        let config_bases = [self.config.clone(), self.home.as_ref().map(|home| home.join(".config"))];
        for base in config_bases.into_iter().flatten() {
            push_pair(&mut pairs, &base, LEGACY_APP_DIR_NAME, APP_DIR_NAME);
        }
        if let Some(cache) = &self.cache {
            push_pair(&mut pairs, cache, LEGACY_APP_DIR_NAME, APP_DIR_NAME);
            push_pair(&mut pairs, cache, LEGACY_IDENTIFIER, IDENTIFIER);
        }
        if let Some(local_data) = &self.local_data {
            push_pair(&mut pairs, local_data, LEGACY_IDENTIFIER, IDENTIFIER);
        }
        pairs
    }
}

fn push_pair(pairs: &mut Vec<(PathBuf, PathBuf)>, base: &Path, old: &str, new: &str) {
    let pair = (base.join(old), base.join(new));
    if !pairs.contains(&pair) {
        pairs.push(pair);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Renamed in place.
    Moved,
    /// Renaming was not possible, so the contents were copied. `old_removed` is false when the
    /// original could not be deleted afterwards.
    Copied { old_removed: bool },
    /// Both exist; the current one is used and the earlier one is left where it is.
    KeptExisting,
    /// Nothing was changed; the reason.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub from: PathBuf,
    pub to: PathBuf,
    pub outcome: Outcome,
}

/// What `migrate` did. Empty when there was nothing from an earlier install.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub steps: Vec<Step>,
}

impl Report {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn outcome_for(&self, from: &Path) -> Option<&Outcome> {
        self.steps.iter().find(|step| step.from == from).map(|step| &step.outcome)
    }

    pub fn failures(&self) -> Vec<&Step> {
        self.steps.iter().filter(|step| matches!(step.outcome, Outcome::Failed(_))).collect()
    }

    /// Writes the report to the log. Called once logging is set up, which is after `migrate`.
    pub fn log(&self) {
        for step in &self.steps {
            let (from, to) = (step.from.display(), step.to.display());
            match &step.outcome {
                Outcome::Moved => tracing::info!("Moved Anilili data: {} -> {}", from, to),
                Outcome::Copied { old_removed: true } => {
                    tracing::info!("Copied Anilili data: {} -> {} (the original was removed)", from, to)
                }
                Outcome::Copied { old_removed: false } => tracing::warn!(
                    "Copied Anilili data: {} -> {}; the original could not be removed and can be deleted by hand",
                    from,
                    to
                ),
                Outcome::KeptExisting => tracing::warn!(
                    "Anilili data at {} was not moved: {} already exists and is used as it is",
                    from,
                    to
                ),
                Outcome::Failed(reason) => {
                    tracing::error!("Anilili data at {} could not be moved to {}: {}", from, to, reason)
                }
            }
        }
    }
}

/// True when something other than an empty directory is at `path`.
fn occupied(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Err(_) => false,
        Ok(meta) if meta.is_dir() => fs::read_dir(path).map_or(true, |mut entries| entries.next().is_some()),
        Ok(_) => true,
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
        // Sockets and the like belong to a process that is gone; they are not carried over.
    }
    Ok(())
}

/// Copies the directory `from` to `to` (which must not hold anything) and then deletes `from`.
/// The copy is made under a temporary name next to `to`, so `to` is either complete or absent.
/// `Ok(false)` means the copy is in place but `from` could not be deleted.
pub fn copy_then_remove(from: &Path, to: &Path) -> io::Result<bool> {
    if occupied(to) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", to.display())));
    }
    let name = to.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let staging = to.with_file_name(format!(".{}.migrating-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let copied = copy_tree(from, &staging).and_then(|()| {
        // An empty directory may be in the way; `rename` replaces it.
        fs::rename(&staging, to)
    });
    if let Err(e) = copied {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    Ok(fs::remove_dir_all(from).is_ok())
}

fn move_folder(from: &Path, to: &Path) -> Outcome {
    let renamed = match fs::rename(from, to) {
        Ok(()) => return Outcome::Moved,
        Err(e) => e,
    };
    match copy_then_remove(from, to) {
        Ok(old_removed) => Outcome::Copied { old_removed },
        Err(copy_error) => Outcome::Failed(format!(
            "renaming failed ({}) and so did copying ({})",
            renamed, copy_error
        )),
    }
}

/// Gives the database in `dir` (with its `-wal` and `-shm` companions) its current name. All of
/// them are renamed or none: a database separated from its write-ahead log would lose what
/// was last written to it.
fn rename_database(dir: &Path) -> Option<Step> {
    let (old, new) = (dir.join(LEGACY_DB_FILE_NAME), dir.join(DB_FILE_NAME));
    if !old.is_file() || fs::symlink_metadata(&new).is_ok() {
        return None;
    }
    let with = |path: &Path, suffix: &str| {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };
    let step = |outcome| Some(Step { from: old.clone(), to: new.clone(), outcome });
    if ["-wal", "-shm"].iter().any(|suffix| fs::symlink_metadata(with(&new, suffix)).is_ok()) {
        return step(Outcome::Failed("files of another database are in the way".to_string()));
    }

    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in ["-wal", "-shm", ""] {
        let (from, to) = (with(&old, suffix), with(&new, suffix));
        if !from.exists() {
            continue;
        }
        if let Err(e) = fs::rename(&from, &to) {
            for (from, to) in done.iter().rev() {
                let _ = fs::rename(to, from);
            }
            return step(Outcome::Failed(e.to_string()));
        }
        done.push((from, to));
    }
    step(Outcome::Moved)
}

fn rename_log(dir: &Path) -> Option<Step> {
    let (old, new) = (dir.join(LEGACY_LOG_FILE_NAME), dir.join(LOG_FILE_NAME));
    if !old.is_file() || fs::symlink_metadata(&new).is_ok() {
        return None;
    }
    let outcome = match fs::rename(&old, &new) {
        Ok(()) => Outcome::Moved,
        Err(e) => Outcome::Failed(e.to_string()),
    };
    Some(Step { from: old, to: new, outcome })
}

/// Moves what an install under the earlier name left in `bases` to the current names: each
/// folder that exists under its earlier name and not (or only as an empty folder) under its
/// current one. Nothing that already exists under a current name is replaced or merged into.
/// Never fails as a whole: whatever could not be moved is in the report and stays where it was.
pub fn migrate(bases: &BaseDirs) -> Report {
    let mut report = Report::default();
    for (from, to) in bases.folders() {
        let is_dir = fs::metadata(&from).map_or(false, |meta| meta.is_dir());
        if !is_dir {
            continue;
        }
        let outcome = if occupied(&to) { Outcome::KeptExisting } else { move_folder(&from, &to) };
        report.steps.push(Step { from, to, outcome });
    }
    // Also on later starts, so a rename that did not work once is tried again.
    for (_, dir) in bases.data_dirs() {
        if dir.is_dir() {
            report.steps.extend(rename_database(&dir));
            report.steps.extend(rename_log(&dir));
        }
    }
    report
}

/// Finished downloads are recorded with their full path. Rows that point into a data folder
/// under its earlier name are pointed at the same file in the current one, when the file is
/// there and no longer at the recorded place. Returns how many rows were changed.
pub async fn repoint_download_paths(pool: &SqlitePool, bases: &BaseDirs) -> Result<usize, sqlx::Error> {
    let pairs = bases.data_dirs();
    let rows = sqlx::query("SELECT id, file_path FROM downloads WHERE file_path IS NOT NULL AND file_path != ''")
        .fetch_all(pool)
        .await?;
    let mut changed = 0;
    for row in rows {
        let id: String = row.try_get("id")?;
        let recorded: String = row.try_get("file_path")?;
        let recorded_path = Path::new(&recorded);
        let moved = pairs.iter().find_map(|(old, new)| {
            let rest = recorded_path.strip_prefix(old).ok()?;
            Some(new.join(rest))
        });
        let Some(moved) = moved else { continue };
        if recorded_path.exists() || !moved.exists() {
            continue;
        }
        sqlx::query("UPDATE downloads SET file_path = ?1 WHERE id = ?2")
            .bind(moved.to_string_lossy().as_ref())
            .bind(&id)
            .execute(pool)
            .await?;
        changed += 1;
    }
    Ok(changed)
}
