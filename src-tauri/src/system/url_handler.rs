//! Makes `anix://` (and the older `anilili://`) links open the AppImage.
//!
//! An AppImage installs nothing, so without this no program handles the scheme and links such
//! as the poster menu's "Copy link" do nothing outside the app. On the first run of an AppImage
//! this writes one hidden `.desktop` file into `$XDG_DATA_HOME/applications` that declares the
//! schemes and starts the AppImage with the link. It is written again only when the AppImage it
//! names no longer exists (the file was moved), and a file of the same name that Anix did not
//! write is never touched. RPM and DEB packages install their own entry, so they skip this.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "anix-url-handler.desktop";
/// Marks the file as ours and records which AppImage it starts, so a later run can tell
/// whether that file still exists without parsing the quoted `Exec` line.
const TARGET_KEY: &str = "X-Anix-AppImage=";
const SCHEMES: [&str; 2] = ["x-scheme-handler/anix", "x-scheme-handler/anilili"];

/// `<data home>/applications/anix-url-handler.desktop`.
pub fn handler_path(data_home: &Path) -> PathBuf {
    data_home.join("applications").join(FILE_NAME)
}

/// The handler entry for `appimage`, or `None` for a path with a line break, which would add
/// keys of its own to the file, or one that is not UTF-8, which could not be recorded as is.
pub fn handler_entry(appimage: &Path) -> Option<String> {
    let path = appimage.to_str()?;
    if path.contains(|c: char| c.is_control()) {
        return None;
    }
    Some(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Anix\n\
         Exec={} %u\n\
         Icon=anix\n\
         Terminal=false\n\
         NoDisplay=true\n\
         MimeType={};\n\
         {}{}\n",
        super::autostart::quote_exec_arg(path),
        SCHEMES.join(";"),
        TARGET_KEY,
        path
    ))
}

/// Whether the handler file must be (re)written, given its current text: when there is none,
/// or when it is ours and the AppImage it names is gone.
pub fn needs_writing(existing: Option<&str>, exists: impl Fn(&Path) -> bool) -> bool {
    let Some(text) = existing else { return true };
    match text.lines().find_map(|line| line.strip_prefix(TARGET_KEY)) {
        Some(target) => !exists(Path::new(target)),
        // Not written by Anix.
        None => false,
    }
}

/// Writes the handler for `appimage` under `data_home` when [`needs_writing`] says so.
/// Returns whether it wrote the file.
pub fn register(data_home: &Path, appimage: &Path) -> io::Result<bool> {
    let path = handler_path(data_home);
    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        // Unreadable or not text, so not a file Anix wrote: left alone.
        Err(e) => {
            tracing::debug!("{} is left alone: {}", path.display(), e);
            return Ok(false);
        }
    };
    if !needs_writing(existing.as_deref(), Path::exists) {
        return Ok(false);
    }
    let Some(entry) = handler_entry(appimage) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the AppImage path has a line break or is not UTF-8"));
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("desktop.tmp{}", std::process::id()));
    fs::write(&tmp, entry)?;
    fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(true)
}

/// Called once from `setup`: registers the handler when running as an AppImage. When it wrote
/// the file, asks the desktop to pick it up (`update-desktop-database`, `xdg-mime default`),
/// best effort and on a thread of its own, since both are optional host tools.
pub fn register_for_appimage() {
    let Some(appimage) = std::env::var_os("APPIMAGE").filter(|p| !p.is_empty()).map(PathBuf::from) else {
        return;
    };
    let Some(data_home) = dirs::data_dir() else { return };
    match register(&data_home, &appimage) {
        Ok(false) => {}
        Ok(true) => {
            tracing::info!("anix:// links now open this AppImage");
            let applications = data_home.join("applications");
            std::thread::spawn(move || {
                let mut update = crate::commands::player::sanitized_command("update-desktop-database");
                update.arg(&applications);
                let mut default = crate::commands::player::sanitized_command("xdg-mime");
                default.arg("default").arg(FILE_NAME).args(SCHEMES);
                for mut cmd in [update, default] {
                    cmd.stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    if let Err(e) = cmd.status() {
                        tracing::debug!("{:?} did not run: {}", cmd.get_program(), e);
                    }
                }
            });
        }
        Err(e) => tracing::warn!("anix:// links could not be registered: {}", e),
    }
}
