//! What the desktop says about window buttons and the accent colour, so the custom title
//! bar and the theme can follow it. Every lookup is optional and falls back quietly.

use crate::commands::player::sanitized_command;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

/// The window buttons on each side of the title bar, in the order they are drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ButtonLayout {
    pub left: Vec<WindowButton>,
    pub right: Vec<WindowButton>,
}

impl Default for ButtonLayout {
    fn default() -> Self {
        Self {
            left: Vec::new(),
            right: vec![WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesktopAppearance {
    pub button_layout: ButtonLayout,
    /// Where the layout came from: "gnome", "kde" or "default".
    pub button_layout_source: String,
    /// The desktop accent colour as `#rrggbb`, when the desktop has one.
    pub accent_color: Option<String>,
    /// "portal", "gnome", "kde", or null.
    pub accent_source: Option<String>,
}

fn gnome_button(name: &str) -> Option<WindowButton> {
    match name.trim() {
        "minimize" => Some(WindowButton::Minimize),
        "maximize" => Some(WindowButton::Maximize),
        "close" => Some(WindowButton::Close),
        _ => None,
    }
}

fn kde_button(letter: char) -> Option<WindowButton> {
    match letter {
        'I' => Some(WindowButton::Minimize),
        'A' => Some(WindowButton::Maximize),
        'X' => Some(WindowButton::Close),
        _ => None,
    }
}

/// Parses the `button-layout` value of `org.gnome.desktop.wm.preferences`, as printed by
/// `gsettings get` (`'appmenu:minimize,maximize,close'`). Entries other than the three
/// window buttons (appmenu, icon, spacer) are left out. `None` when the value is not a layout.
pub fn parse_gnome_button_layout(raw: &str) -> Option<ButtonLayout> {
    if raw.trim().is_empty() {
        return None;
    }
    let value = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    let (left, right) = value.split_once(':').unwrap_or((value, ""));
    let side = |list: &str| list.split(',').filter_map(gnome_button).collect::<Vec<_>>();
    Some(ButtonLayout {
        left: side(left),
        right: side(right),
    })
}

/// Reads `ButtonsOnLeft` / `ButtonsOnRight` from the `[org.kde.kdecoration2]` group of a
/// kwinrc. KWin's defaults ("MS" and "HIAX") apply to a missing key. `None` when the group
/// is absent, which means KWin has never been configured and the defaults hold anyway.
pub fn parse_kwinrc_button_layout(content: &str) -> Option<ButtonLayout> {
    let mut in_group = false;
    let mut seen_group = false;
    let mut left: Option<String> = None;
    let mut right: Option<String> = None;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[org.kde.kdecoration2]";
            seen_group |= in_group;
            continue;
        }
        if !in_group {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "ButtonsOnLeft" => left = Some(value.trim().to_string()),
                "ButtonsOnRight" => right = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }
    if !seen_group {
        return None;
    }
    let side = |letters: &str| letters.chars().filter_map(kde_button).collect::<Vec<_>>();
    Some(ButtonLayout {
        left: side(left.as_deref().unwrap_or("MS")),
        right: side(right.as_deref().unwrap_or("HIAX")),
    })
}

/// The colours GNOME (libadwaita) uses for each `accent-color` name.
pub fn gnome_accent_hex(raw: &str) -> Option<&'static str> {
    match raw.trim().trim_matches('\'') {
        "blue" => Some("#3584e4"),
        "teal" => Some("#2190a4"),
        "green" => Some("#3a944a"),
        "yellow" => Some("#c88800"),
        "orange" => Some("#ed5b00"),
        "red" => Some("#e62d42"),
        "pink" => Some("#d56199"),
        "purple" => Some("#9141ac"),
        "slate" => Some("#6f8396"),
        _ => None,
    }
}

fn to_hex(r: f64, g: f64, b: f64) -> String {
    let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(r), channel(g), channel(b))
}

/// Parses the reply of the portal's `Settings.ReadOne("org.freedesktop.appearance",
/// "accent-color")` as printed by gdbus: `(<(0.2078, 0.5176, 0.8941)>,)`. The portal reports
/// "no accent" with values outside 0..1.
pub fn parse_portal_accent(raw: &str) -> Option<String> {
    let numbers: Vec<f64> = raw
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
        .filter(|part| !part.is_empty() && part.chars().any(|c| c.is_ascii_digit()))
        .filter_map(|part| part.parse::<f64>().ok())
        .collect();
    if numbers.len() < 3 {
        return None;
    }
    let (r, g, b) = (numbers[0], numbers[1], numbers[2]);
    if [r, g, b].iter().any(|v| !(0.0..=1.0).contains(v)) {
        return None;
    }
    Some(to_hex(r, g, b))
}

/// `AccentColor=61,174,233` from the `[General]` group of kdeglobals.
pub fn parse_kdeglobals_accent(content: &str) -> Option<String> {
    let mut in_general = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_general = line == "[General]";
            continue;
        }
        if !in_general {
            continue;
        }
        let Some(value) = line.strip_prefix("AccentColor=") else {
            continue;
        };
        let parts: Vec<u8> = value
            .split(',')
            .filter_map(|part| part.trim().parse::<u8>().ok())
            .collect();
        if parts.len() >= 3 {
            return Some(format!("#{:02x}{:02x}{:02x}", parts[0], parts[1], parts[2]));
        }
        if let Some(hex) = value.trim().strip_prefix('#') {
            if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(format!("#{}", hex.to_ascii_lowercase()));
            }
        }
    }
    None
}

async fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut command = tokio::process::Command::from(sanitized_command(program));
    command.args(args).kill_on_drop(true);
    let output = tokio::time::timeout(LOOKUP_TIMEOUT, command.output()).await.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn config_file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;
    Some(base.join(name))
}

fn is_kde() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|value| value.split(':').any(|part| part.eq_ignore_ascii_case("KDE")))
        .unwrap_or(false)
}

async fn gnome_layout() -> Option<ButtonLayout> {
    let raw = run("gsettings", &["get", "org.gnome.desktop.wm.preferences", "button-layout"]).await?;
    parse_gnome_button_layout(&raw)
}

fn kde_layout() -> Option<ButtonLayout> {
    let content = std::fs::read_to_string(config_file("kwinrc")?).ok()?;
    parse_kwinrc_button_layout(&content)
}

async fn portal_accent() -> Option<String> {
    let raw = run(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.ReadOne",
            "org.freedesktop.appearance",
            "accent-color",
        ],
    )
    .await?;
    parse_portal_accent(&raw)
}

async fn gnome_accent() -> Option<String> {
    let raw = run("gsettings", &["get", "org.gnome.desktop.interface", "accent-color"]).await?;
    gnome_accent_hex(&raw).map(String::from)
}

fn kde_accent() -> Option<String> {
    let content = std::fs::read_to_string(config_file("kdeglobals")?).ok()?;
    parse_kdeglobals_accent(&content)
}

/// Window-button layout and accent colour of the running desktop.
#[tauri::command]
pub async fn get_desktop_appearance() -> Result<DesktopAppearance, String> {
    let kde = is_kde();
    let (button_layout, button_layout_source) = if kde {
        match kde_layout() {
            Some(layout) => (layout, "kde"),
            None => (ButtonLayout::default(), "kde"),
        }
    } else {
        match gnome_layout().await {
            Some(layout) => (layout, "gnome"),
            None => (ButtonLayout::default(), "default"),
        }
    };

    let mut accent = portal_accent().await.map(|hex| (hex, "portal"));
    if accent.is_none() {
        accent = if kde {
            kde_accent().map(|hex| (hex, "kde"))
        } else {
            gnome_accent().await.map(|hex| (hex, "gnome"))
        };
    }

    Ok(DesktopAppearance {
        button_layout,
        button_layout_source: button_layout_source.to_string(),
        accent_color: accent.as_ref().map(|(hex, _)| hex.clone()),
        accent_source: accent.map(|(_, source)| source.to_string()),
    })
}
