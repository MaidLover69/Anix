//! MPRIS (`org.mpris.MediaPlayer2`) on the session bus for the built-in player, so media keys,
//! desktop widgets and `playerctl` can see and drive it. The Watch screen reports its state
//! with `mpris_update` and withdraws it with `mpris_clear`; requests from the bus reach the
//! player as `MPRIS_EVENT` events.

use std::collections::HashMap;
use std::time::Instant;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{interface, Connection};

pub const MPRIS_EVENT: &str = "mpris-control";

const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const BUS_NAME: &str = "org.mpris.MediaPlayer2.anix";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// What the player shows, as the frontend reports it. Times are in seconds.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MprisState {
    pub title: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub art_url: Option<String>,
    /// Identifies the episode; anything, it is turned into an object path.
    pub track: String,
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
    pub rate: f64,
    pub volume: f64,
    pub can_next: bool,
    pub can_previous: bool,
    /// The position jumped (a seek), which MPRIS announces with the `Seeked` signal.
    #[serde(default)]
    pub seeked: bool,
}

/// A request from the bus, handed to the player.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum Control {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Relative seek in seconds.
    Seek { offset: f64 },
    /// Absolute position in seconds.
    SetPosition { position: f64 },
    SetVolume { volume: f64 },
    SetRate { rate: f64 },
}

fn seconds_to_us(seconds: f64) -> i64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1_000_000.0).round() as i64
    } else {
        0
    }
}

/// An object path for a track id: letters, digits and `_` only, as D-Bus requires.
pub fn track_path(track: &str) -> String {
    let cleaned: String = track
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(120)
        .collect();
    if cleaned.is_empty() {
        NO_TRACK.to_string()
    } else {
        format!("/io/anix/episode/{}", cleaned)
    }
}

/// The `Metadata` map for `state`.
pub fn metadata(state: &MprisState) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();
    let mut put = |key: &str, value: Value<'_>| {
        if let Ok(owned) = OwnedValue::try_from(value) {
            map.insert(key.to_string(), owned);
        }
    };
    if let Ok(path) = ObjectPath::try_from(track_path(&state.track)) {
        put("mpris:trackid", Value::from(path));
    }
    if state.duration > 0.0 {
        put("mpris:length", Value::from(seconds_to_us(state.duration)));
    }
    put("xesam:title", Value::from(state.title.clone()));
    if let Some(album) = state.album.as_ref().filter(|a| !a.is_empty()) {
        put("xesam:album", Value::from(album.clone()));
    }
    if let Some(art) = state.art_url.as_ref().filter(|a| a.starts_with("https://") || a.starts_with("http://")) {
        put("mpris:artUrl", Value::from(art.clone()));
    }
    map
}

/// Where requests from the bus go: the app's event bus, or a test's collector.
type Sender = std::sync::Arc<dyn Fn(Control) + Send + Sync>;
type Raiser = std::sync::Arc<dyn Fn() + Send + Sync>;

struct Root {
    raise: Raiser,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        (self.raise)();
    }

    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        "Anix".to_string()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "anix".to_string()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    send: Sender,
    state: MprisState,
    /// When `state.position` was reported; the position moves on from there while playing.
    reported_at: Instant,
}

impl Player {
    fn send(&self, control: Control) {
        (self.send)(control);
    }

    fn current_position(&self) -> f64 {
        let mut position = self.state.position;
        if self.state.playing {
            position += self.reported_at.elapsed().as_secs_f64() * self.state.rate.max(0.0);
        }
        if self.state.duration > 0.0 {
            position = position.min(self.state.duration);
        }
        position
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.send(Control::Next);
    }

    fn previous(&self) {
        self.send(Control::Previous);
    }

    fn pause(&self) {
        self.send(Control::Pause);
    }

    fn play_pause(&self) {
        self.send(Control::PlayPause);
    }

    fn stop(&self) {
        self.send(Control::Stop);
    }

    fn play(&self) {
        self.send(Control::Play);
    }

    /// `offset` is in microseconds.
    fn seek(&self, offset: i64) {
        self.send(Control::Seek { offset: offset as f64 / 1_000_000.0 });
    }

    /// Ignored unless `track_id` is the current track, as the specification asks.
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        if track_id.as_str() != track_path(&self.state.track) || position < 0 {
            return;
        }
        let seconds = position as f64 / 1_000_000.0;
        if self.state.duration > 0.0 && seconds > self.state.duration {
            return;
        }
        self.send(Control::SetPosition { position: seconds });
    }

    fn open_uri(&self, _uri: String) {}

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        if self.state.playing { "Playing" } else { "Paused" }.to_string()
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        if self.state.rate > 0.0 { self.state.rate } else { 1.0 }
    }

    #[zbus(property)]
    fn set_rate(&mut self, rate: f64) {
        if rate > 0.0 && rate.is_finite() {
            self.send(Control::SetRate { rate: rate.clamp(0.25, 4.0) });
        }
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        0.25
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        4.0
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.state)
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state.volume.clamp(0.0, 1.0)
    }

    #[zbus(property)]
    fn set_volume(&mut self, volume: f64) {
        if volume.is_finite() {
            self.send(Control::SetVolume { volume: volume.clamp(0.0, 1.0) });
        }
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        seconds_to_us(self.current_position())
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.state.can_next
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.state.can_previous
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.state.duration > 0.0
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
}

static CONNECTION: Lazy<tokio::sync::Mutex<Option<Connection>>> = Lazy::new(|| tokio::sync::Mutex::new(None));

async fn export(send: Sender, raise: Raiser, state: MprisState) -> zbus::Result<Connection> {
    let player = Player { send, state, reported_at: Instant::now() };
    let connection = zbus::connection::Builder::session()?
        .serve_at(OBJECT_PATH, Root { raise })?
        .serve_at(OBJECT_PATH, player)?
        .build()
        .await?;
    // A second copy of the app (a development build next to the installed one) gets its own name.
    if connection.request_name(BUS_NAME).await.is_err() {
        connection
            .request_name(format!("{}.instance{}", BUS_NAME, std::process::id()))
            .await?;
    }
    Ok(connection)
}

/// Exports the player on the session bus, or updates what it shows.
#[tauri::command]
pub async fn mpris_update(app: AppHandle, state: MprisState) -> Result<(), String> {
    let mut guard = CONNECTION.lock().await;
    let Some(connection) = guard.as_ref() else {
        let events = app.clone();
        let send: Sender = std::sync::Arc::new(move |control| {
            let _ = events.emit(MPRIS_EVENT, control);
        });
        let raise: Raiser = std::sync::Arc::new(move || {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        });
        *guard = Some(export(send, raise, state).await.map_err(|e| e.to_string())?);
        return Ok(());
    };

    update(connection, state).await.map_err(|e| e.to_string())
}

/// Replaces what the exported player shows and announces what changed.
async fn update(connection: &Connection, state: MprisState) -> zbus::Result<()> {
    let iface = connection
        .object_server()
        .interface::<_, Player>(OBJECT_PATH)
        .await?;
    let emitter = iface.signal_emitter().clone();
    let mut player = iface.get_mut().await;
    let old = std::mem::replace(&mut player.state, state);
    player.reported_at = Instant::now();
    let new = &player.state;

    if old.playing != new.playing {
        let _ = player.playback_status_changed(&emitter).await;
    }
    if old.title != new.title
        || old.album != new.album
        || old.art_url != new.art_url
        || old.track != new.track
        || (old.duration - new.duration).abs() >= 0.5
    {
        let _ = player.metadata_changed(&emitter).await;
        let _ = player.can_seek_changed(&emitter).await;
    }
    if old.rate != new.rate {
        let _ = player.rate_changed(&emitter).await;
    }
    if old.volume != new.volume {
        let _ = player.volume_changed(&emitter).await;
    }
    if old.can_next != new.can_next {
        let _ = player.can_go_next_changed(&emitter).await;
    }
    if old.can_previous != new.can_previous {
        let _ = player.can_go_previous_changed(&emitter).await;
    }
    if new.seeked {
        let _ = Player::seeked(&emitter, seconds_to_us(new.position)).await;
    }
    Ok(())
}

/// Withdraws the player from the session bus.
#[tauri::command]
pub async fn mpris_clear() -> Result<(), String> {
    let Some(connection) = CONNECTION.lock().await.take() else { return Ok(()) };
    let _ = connection.object_server().remove::<Player, _>(OBJECT_PATH).await;
    let _ = connection.object_server().remove::<Root, _>(OBJECT_PATH).await;
    for name in [BUS_NAME.to_string(), format!("{}.instance{}", BUS_NAME, std::process::id())] {
        if let Ok(name) = zbus::names::WellKnownName::try_from(name) {
            let _ = connection.release_name(name).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> MprisState {
        MprisState {
            title: "Frieren – Episode 3".into(),
            album: Some("Frieren".into()),
            art_url: Some("https://img.example/cover.jpg".into()),
            track: "154587-3".into(),
            duration: 1420.0,
            position: 10.0,
            playing: true,
            rate: 1.0,
            volume: 0.8,
            can_next: true,
            can_previous: false,
            seeked: false,
        }
    }

    #[test]
    fn track_paths_are_valid_object_paths() {
        assert_eq!(track_path("154587-3.5"), "/io/anix/episode/154587_3_5");
        assert_eq!(track_path(""), NO_TRACK);
        assert!(ObjectPath::try_from(track_path("a/b c–d")).is_ok());
    }

    #[test]
    fn metadata_carries_title_length_and_art() {
        let map = metadata(&state());
        assert_eq!(String::try_from(map["xesam:title"].clone()).unwrap(), "Frieren – Episode 3");
        assert_eq!(i64::try_from(map["mpris:length"].clone()).unwrap(), 1_420_000_000);
        assert_eq!(String::try_from(map["mpris:artUrl"].clone()).unwrap(), "https://img.example/cover.jpg");
        assert!(map.contains_key("mpris:trackid"));
    }

    #[test]
    fn unknown_length_and_odd_art_urls_are_left_out() {
        let mut s = state();
        s.duration = f64::NAN;
        s.art_url = Some("file:///etc/passwd".into());
        let map = metadata(&s);
        assert!(!map.contains_key("mpris:length"));
        assert!(!map.contains_key("mpris:artUrl"));
    }

    fn playerctl(args: &[&str]) -> String {
        let out = std::process::Command::new("playerctl")
            .args(["--player", "anix"])
            .args(args)
            .output()
            .expect("playerctl is installed");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Exports on the real session bus and drives it with `playerctl`
    /// (`cargo test --offline mpris -- --ignored`).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn playerctl_sees_and_drives_the_player() {
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = received.clone();
        let send: Sender = std::sync::Arc::new(move |c| sink.lock().unwrap().push(c));
        let connection = export(send, std::sync::Arc::new(|| {}), state()).await.unwrap();

        let output = tokio::task::spawn_blocking(|| {
            let status = playerctl(&["status"]);
            let title = playerctl(&["metadata", "xesam:title"]);
            let art = playerctl(&["metadata", "mpris:artUrl"]);
            let position: f64 = playerctl(&["position"]).parse().unwrap_or(-1.0);
            playerctl(&["play-pause"]);
            playerctl(&["next"]);
            playerctl(&["position", "30"]);
            playerctl(&["position", "5-"]);
            (status, title, art, position)
        })
        .await
        .unwrap();
        // A second client sees the change announced, without asking for it.
        use futures_util::StreamExt;
        let listener = Connection::session().await.unwrap();
        let properties = zbus::fdo::PropertiesProxy::builder(&listener)
            .destination(BUS_NAME)
            .unwrap()
            .path(OBJECT_PATH)
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut changes = properties.receive_properties_changed().await.unwrap();

        let mut paused = state();
        paused.playing = false;
        paused.title = "Frieren – Episode 4".into();
        update(&connection, paused).await.unwrap();
        let announced = tokio::time::timeout(std::time::Duration::from_secs(3), changes.next())
            .await
            .expect("a PropertiesChanged signal")
            .unwrap();
        let args = announced.args().unwrap();
        assert_eq!(args.interface_name().as_str(), "org.mpris.MediaPlayer2.Player");
        assert!(args.changed_properties().contains_key("PlaybackStatus"));
        let after = tokio::task::spawn_blocking(|| (playerctl(&["status"]), playerctl(&["metadata", "xesam:title"])))
            .await
            .unwrap();
        drop(connection);

        assert_eq!(after, ("Paused".to_string(), "Frieren – Episode 4".to_string()));
        assert_eq!(output.0, "Playing");
        assert_eq!(output.1, "Frieren – Episode 3");
        assert_eq!(output.2, "https://img.example/cover.jpg");
        assert!(output.3 >= 10.0 && output.3 < 15.0, "position {}", output.3);
        let received = received.lock().unwrap().clone();
        assert_eq!(
            received,
            vec![
                Control::PlayPause,
                Control::Next,
                Control::SetPosition { position: 30.0 },
                Control::Seek { offset: -5.0 },
            ]
        );
    }

    #[test]
    fn controls_serialize_with_an_action_tag() {
        let json = serde_json::to_value(Control::Seek { offset: -10.0 }).unwrap();
        assert_eq!(json, serde_json::json!({ "action": "seek", "offset": -10.0 }));
        let json = serde_json::to_value(Control::PlayPause).unwrap();
        assert_eq!(json, serde_json::json!({ "action": "playPause" }));
    }
}
