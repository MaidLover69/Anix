//! The TLS priority string for the AppImage. The bundled GnuTLS was built on Fedora and asks
//! for the system crypto policy, which only Fedora and its relatives ship. Elsewhere every TLS
//! connection the webview makes fails, so covers and banners stay empty.

use std::ffi::OsStr;
use std::path::Path;

pub const PRIORITY_VAR: &str = "G_TLS_GNUTLS_PRIORITY";
pub const PRIORITY_VALUE: &str = "NORMAL";
/// Written by Fedora's crypto-policies package; the bundled GnuTLS reads it.
pub const CRYPTO_POLICY_FILE: &str = "/etc/crypto-policies/back-ends/gnutls.config";

/// True when the priority must be set: running as an AppImage, the policy file is missing,
/// and the user has not set the variable themselves.
pub fn needs_priority(appimage: Option<&OsStr>, policy_file_exists: bool, current: Option<&OsStr>) -> bool {
    appimage.is_some_and(|path| !path.is_empty()) && !policy_file_exists && current.is_none()
}

/// Sets the priority when [`needs_priority`] says so. Must run before any thread starts, so
/// `main` calls it before Tauri. Returns whether it was set.
pub fn apply() -> bool {
    let needed = needs_priority(
        std::env::var_os("APPIMAGE").as_deref(),
        Path::new(CRYPTO_POLICY_FILE).exists(),
        std::env::var_os(PRIORITY_VAR).as_deref(),
    );
    if needed {
        std::env::set_var(PRIORITY_VAR, PRIORITY_VALUE);
    }
    needed
}
