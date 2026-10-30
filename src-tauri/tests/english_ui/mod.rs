//! For tests that check backend text in English. `i18n::current()` reads `app_language` from
//! settings.json and the locale, so without this the owner's own choice (or LANG) would change
//! the text, and the test would read the real ~/.config/anix.

use std::sync::Once;

/// Points the settings lookup at an empty scratch home and the locale at C, once per test binary.
/// Call it first in every test that expects English from `i18n::current()`.
pub fn english_ui() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let home = std::env::temp_dir().join("anix-english-ui");
        std::fs::create_dir_all(&home).expect("scratch home");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        // "C.*" makes the locale name no language, so the default English is used.
        std::env::set_var("LC_ALL", "C.UTF-8");
        anix_lib::i18n::forget();
    });
}
