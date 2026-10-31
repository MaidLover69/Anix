//! "Start with the desktop session": an XDG autostart entry in `~/.config/autostart/`.
//!
//! The entry starts the app with [`super::MINIMIZED_ARG`], so it opens in the tray. For the
//! AppImage the entry points at the AppImage file (`$APPIMAGE`), not at the binary inside its
//! temporary mount.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "anix.desktop";
/// Marks entries this app wrote, so a hand-made file of the same name is never removed.
const MARKER: &str = "X-Anix-Autostart=true";

/// `<config home>/autostart/anix.desktop`.
pub fn entry_path(config_home: &Path) -> PathBuf {
    config_home.join("autostart").join(FILE_NAME)
}

/// What the entry starts: the AppImage when running from one, else this executable.
pub fn exec_target(appimage: Option<std::ffi::OsString>, current_exe: Option<PathBuf>) -> Option<PathBuf> {
    appimage
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or(current_exe)
}

/// The running program's target, from the environment.
pub fn current_exec_target() -> Option<PathBuf> {
    exec_target(std::env::var_os("APPIMAGE"), std::env::current_exe().ok())
}

/// One argument of an `Exec` key, quoted as the Desktop Entry Specification asks: arguments
/// with reserved characters go in double quotes with `"`, `` ` ``, `$` and `\` escaped; then
/// the string-value escaping doubles every backslash, and `%` is written `%%`.
pub fn quote_exec_arg(arg: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')', '`',
    ];
    let quoted = if arg.is_empty() || arg.contains(RESERVED) {
        let mut out = String::from("\"");
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    } else {
        arg.to_string()
    };
    quoted.replace('\\', "\\\\").replace('%', "%%")
}

/// The autostart entry for `exec`.
pub fn desktop_entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Anix\n\
         Comment=Starts Anix in the tray when you log in\n\
         Exec={} {}\n\
         Icon=anix\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         {}\n",
        quote_exec_arg(&exec.to_string_lossy()),
        super::MINIMIZED_ARG,
        MARKER
    )
}

/// Writes (when `exec` is given) or removes the entry under `config_home`. Only an entry this
/// app wrote is removed or replaced. Writing an unchanged entry does nothing.
pub fn set_enabled(config_home: &Path, exec: Option<&Path>) -> io::Result<()> {
    write_ours(&entry_path(config_home), exec.map(desktop_entry).as_deref(), MARKER).map(|_| ())
}

/// Writes `wanted` to `path` (or removes the file with `None`); only a file that is missing or
/// contains `marker` is replaced or removed. Returns whether anything changed.
pub fn write_ours(path: &Path, wanted: Option<&str>, marker: &str) -> io::Result<bool> {
    let existing = fs::read_to_string(path).ok();
    let ours = existing.as_deref().is_none_or(|text| text.contains(marker));
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
                fs::create_dir_all(parent)?;
            }
            let mut tmp = path.as_os_str().to_owned();
            tmp.push(format!(".tmp{}", std::process::id()));
            let tmp = PathBuf::from(tmp);
            fs::write(&tmp, wanted)?;
            fs::rename(&tmp, path).inspect_err(|_| {
                let _ = fs::remove_file(&tmp);
            })?;
            Ok(true)
        }
        None if existing.is_some() && ours => {
            fs::remove_file(path)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Whether this app's entry exists under `config_home`.
pub fn is_installed(config_home: &Path) -> bool {
    fs::read_to_string(entry_path(config_home)).is_ok_and(|text| text.contains(MARKER))
}
