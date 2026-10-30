//! Two-way link with an mpv started for one episode. mpv reports its position over its JSON
//! IPC socket; the position is saved through the same command the built-in player uses, so
//! history, the watched threshold and account sync behave the same, and the open Watch
//! screen is told through `EXTERNAL_PLAYER_EVENT`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub const EXTERNAL_PLAYER_EVENT: &str = "external-player-progress";

const SAVE_INTERVAL: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_RETRY: Duration = Duration::from_millis(150);

/// The episode an external player shows, with what a history row needs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeTarget {
    pub anilist_id: i32,
    pub title: String,
    #[serde(default)]
    pub cover: Option<String>,
    pub episode_number: f64,
    #[serde(default)]
    pub episode_title: Option<String>,
    pub provider: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub anilist_id: i32,
    pub episode_number: f64,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub paused: bool,
    /// The episode was played to its end.
    pub ended: bool,
    /// False once mpv has exited.
    pub running: bool,
}

/// Where mpv's socket goes: the user's runtime directory, or the temp directory without one.
pub fn socket_path(nonce: u128) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute() && d.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("anix-mpv-{}.sock", nonce))
}

/// The commands sent once connected: one property observer per value the sync needs.
pub fn observe_commands() -> String {
    ["time-pos", "duration", "pause", "eof-reached"]
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{{\"command\":[\"observe_property\",{},\"{}\"]}}\n", i + 1, name))
        .collect()
}

/// What mpv has reported so far.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MpvState {
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub ended: bool,
    /// Enough real playback for history (see [`MIN_PLAYED_S`]); before that nothing is saved,
    /// so opening an episode never replaces its saved position or lists it as watched.
    pub played: bool,
    last_position: Option<f64>,
    played_seconds: f64,
}

/// The rule of `src/hooks/watchProgress.ts` (`hasPlayedEnough`, `playedStep`), kept the same
/// for every player: a visit counts after 10 s or 2 % of real playback, whichever comes first,
/// and a forward step above 3 s between readings is a seek, not playback.
const MIN_PLAYED_S: f64 = 10.0;
const MIN_PLAYED_FRACTION: f64 = 0.02;
const MAX_PLAYED_STEP_S: f64 = 3.0;

/// What one IPC line changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    None,
    Position,
    Pause,
    Ended,
}

impl MpvState {
    pub fn apply(&mut self, line: &str) -> Change {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return Change::None;
        };
        match value.get("event").and_then(|e| e.as_str()) {
            Some("property-change") => {}
            Some("end-file") => {
                if value.get("reason").and_then(|r| r.as_str()) == Some("eof") {
                    return self.mark_ended();
                }
                return Change::None;
            }
            _ => return Change::None,
        }
        let data = value.get("data");
        match value.get("name").and_then(|n| n.as_str()) {
            Some("time-pos") => {
                let Some(time) = data.and_then(|d| d.as_f64()).filter(|t| t.is_finite() && *t >= 0.0) else {
                    return Change::None;
                };
                self.position = time;
                if let Some(last) = self.last_position.replace(time) {
                    let step = time - last;
                    if !self.paused && step > 0.0 && step <= MAX_PLAYED_STEP_S {
                        self.played_seconds += step;
                    }
                }
                self.check_played();
                Change::Position
            }
            Some("duration") => {
                if let Some(d) = data.and_then(|d| d.as_f64()).filter(|d| d.is_finite() && *d > 0.0) {
                    self.duration = d;
                    self.check_played();
                }
                Change::None
            }
            Some("pause") => {
                let paused = data.and_then(|d| d.as_bool()).unwrap_or(false);
                if paused == self.paused {
                    return Change::None;
                }
                self.paused = paused;
                Change::Pause
            }
            Some("eof-reached") if data.and_then(|d| d.as_bool()) == Some(true) => self.mark_ended(),
            _ => Change::None,
        }
    }

    fn check_played(&mut self) {
        if self.duration > 0.0 && self.played_seconds >= MIN_PLAYED_S.min(self.duration * MIN_PLAYED_FRACTION) {
            self.played = true;
        }
    }

    fn mark_ended(&mut self) -> Change {
        if self.ended || !self.played || self.duration <= 0.0 {
            return Change::None;
        }
        // mpv's last position is a little before the end; that counts as finished. A stream
        // that broke off far from the end also ends the file, but is not finished.
        if self.position < self.duration - 5.0 {
            return Change::None;
        }
        self.position = self.duration;
        self.ended = true;
        Change::Ended
    }

    /// Position and duration in milliseconds, when there is something worth saving.
    pub fn saveable(&self) -> Option<(i64, i64)> {
        if !self.played || self.duration <= 0.0 {
            return None;
        }
        let position = self.position.clamp(0.0, self.duration);
        Some(((position * 1000.0).round() as i64, (self.duration * 1000.0).round() as i64))
    }
}

async fn connect(path: &Path, child: &mut tokio::process::Child) -> Option<UnixStream> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if let Ok(stream) = UnixStream::connect(path).await {
            return Some(stream);
        }
        if Instant::now() >= deadline || matches!(child.try_wait(), Ok(Some(_))) {
            return None;
        }
        tokio::time::sleep(CONNECT_RETRY).await;
    }
}

struct Sync {
    app: AppHandle,
    target: EpisodeTarget,
    last_saved: Option<i64>,
}

impl Sync {
    async fn save(&mut self, state: &MpvState) {
        let Some((position_ms, duration_ms)) = state.saveable() else { return };
        if self.last_saved.is_some_and(|last| (last - position_ms).abs() < 1000) {
            return;
        }
        self.last_saved = Some(position_ms);
        let t = self.target.clone();
        let result = crate::commands::library::update_watch_progress(
            self.app.clone(),
            self.app.state(),
            t.anilist_id,
            t.title,
            t.cover,
            t.episode_number,
            t.episode_title,
            t.provider,
            t.category,
            position_ms,
            duration_ms,
        )
        .await;
        if let Err(e) = result {
            tracing::warn!("Could not save the mpv position: {}", e);
        }
    }

    fn emit(&self, state: &MpvState, running: bool) {
        let event = ProgressEvent {
            anilist_id: self.target.anilist_id,
            episode_number: self.target.episode_number,
            position_ms: (state.position * 1000.0).round() as i64,
            duration_ms: (state.duration * 1000.0).round() as i64,
            paused: state.paused,
            ended: state.ended,
            running,
        };
        let _ = self.app.emit(EXTERNAL_PLAYER_EVENT, event);
    }
}

/// Follows mpv until it exits, saving its position, then removes the socket.
pub async fn follow(app: AppHandle, mut child: tokio::process::Child, socket: PathBuf, target: EpisodeTarget) {
    let mut sync = Sync { app, target, last_saved: None };
    let mut state = MpvState::default();

    if let Some(stream) = connect(&socket, &mut child).await {
        let (reader, mut writer) = stream.into_split();
        if writer.write_all(observe_commands().as_bytes()).await.is_ok() {
            let mut lines = BufReader::new(reader).lines();
            let mut last_save = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    line = lines.next_line() => {
                        let Ok(Some(line)) = line else { break };
                        match state.apply(&line) {
                            Change::Pause => {
                                if state.paused {
                                    sync.save(&state).await;
                                    last_save = Instant::now();
                                }
                                sync.emit(&state, true);
                            }
                            Change::Ended => {
                                sync.save(&state).await;
                                last_save = Instant::now();
                                sync.emit(&state, true);
                            }
                            Change::Position | Change::None => {}
                        }
                    }
                    _ = tick.tick() => {
                        if !state.paused && last_save.elapsed() >= SAVE_INTERVAL {
                            sync.save(&state).await;
                            sync.emit(&state, true);
                            last_save = Instant::now();
                        }
                    }
                }
            }
        }
    } else {
        tracing::warn!("mpv did not open its IPC socket; its position is not synced");
    }

    let _ = child.wait().await;
    sync.save(&state).await;
    sync.emit(&state, false);
    let _ = std::fs::remove_file(&socket);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(name: &str, data: &str) -> String {
        format!("{{\"event\":\"property-change\",\"id\":1,\"name\":\"{}\",\"data\":{}}}", name, data)
    }

    /// Plays from `from` to `to` seconds, reporting the time every half second like mpv does.
    fn play(s: &mut MpvState, from: f64, to: f64) {
        let mut t = from;
        while t <= to {
            s.apply(&prop("time-pos", &t.to_string()));
            t += 0.5;
        }
    }

    #[test]
    fn nothing_is_saveable_before_ten_seconds_of_playback() {
        let mut s = MpvState::default();
        s.apply(&prop("duration", "1420.5"));
        s.apply(&prop("pause", "false"));
        play(&mut s, 300.0, 301.0);
        assert_eq!(s.saveable(), None, "a second of playback is a visit, not watching");
        play(&mut s, 301.5, 309.5);
        assert_eq!(s.saveable(), None);
        play(&mut s, 310.0, 310.0);
        assert_eq!(s.saveable(), Some((310_000, 1_420_500)));
    }

    #[test]
    fn seeks_do_not_count_as_playback() {
        let mut s = MpvState::default();
        s.apply(&prop("duration", "1400"));
        for t in [100.0, 200.0, 300.0, 400.0, 500.0] {
            s.apply(&prop("time-pos", &t.to_string()));
        }
        assert!(!s.played);
    }

    #[test]
    fn a_short_clip_counts_after_two_percent() {
        let mut s = MpvState::default();
        s.apply(&prop("duration", "100"));
        play(&mut s, 0.0, 1.5);
        assert!(!s.played);
        play(&mut s, 2.0, 2.0);
        assert!(s.played);
    }

    #[test]
    fn seeking_while_paused_is_not_playback() {
        let mut s = MpvState::default();
        s.apply(&prop("duration", "100"));
        assert_eq!(s.apply(&prop("pause", "true")), Change::Pause);
        s.apply(&prop("time-pos", "10"));
        s.apply(&prop("time-pos", "50"));
        assert!(!s.played);
        assert_eq!(s.apply(&prop("pause", "true")), Change::None);
    }

    #[test]
    fn null_and_unrelated_lines_are_ignored() {
        let mut s = MpvState::default();
        assert_eq!(s.apply(&prop("time-pos", "null")), Change::None);
        assert_eq!(s.apply("{\"request_id\":0,\"error\":\"success\"}"), Change::None);
        assert_eq!(s.apply("not json"), Change::None);
        assert_eq!(s.position, 0.0);
    }

    #[test]
    fn the_end_of_the_file_counts_as_finished() {
        let mut s = MpvState::default();
        s.apply(&prop("duration", "1400"));
        play(&mut s, 1385.0, 1398.0);
        s.apply(&prop("time-pos", "1398.4"));
        assert_eq!(s.apply("{\"event\":\"end-file\",\"reason\":\"eof\"}"), Change::Ended);
        assert_eq!(s.saveable(), Some((1_400_000, 1_400_000)));
        assert_eq!(s.apply(&prop("eof-reached", "true")), Change::None);
    }

    #[test]
    fn quitting_mid_episode_is_not_the_end() {
        let mut s = MpvState::default();
        s.apply(&prop("duration", "1400"));
        play(&mut s, 180.0, 200.0);
        assert_eq!(s.apply("{\"event\":\"end-file\",\"reason\":\"quit\"}"), Change::None);
        assert_eq!(s.saveable(), Some((200_000, 1_400_000)));
    }

    #[test]
    fn a_stream_that_breaks_off_is_not_finished() {
        let mut s = MpvState::default();
        s.apply(&prop("duration", "1400"));
        play(&mut s, 180.0, 200.0);
        assert_eq!(s.apply("{\"event\":\"end-file\",\"reason\":\"eof\"}"), Change::None);
        assert_eq!(s.saveable(), Some((200_000, 1_400_000)));
    }

    /// Runs a real mpv (`cargo test --offline mpv_ipc -- --ignored`).
    #[tokio::test]
    #[ignore]
    async fn follows_a_real_mpv() {
        let socket = socket_path(std::process::id() as u128);
        let mut child = tokio::process::Command::new("mpv")
            .args(["--no-config", "--vo=null", "--ao=null", "--start=2"])
            .arg(format!("--input-ipc-server={}", socket.display()))
            .arg("av://lavfi:testsrc=duration=12:rate=10")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("mpv is installed");
        let stream = connect(&socket, &mut child).await.expect("mpv opened its socket");
        let (reader, mut writer) = stream.into_split();
        writer.write_all(observe_commands().as_bytes()).await.unwrap();
        let mut lines = BufReader::new(reader).lines();
        let mut state = MpvState::default();
        let mut first = None;
        let mut changes = Vec::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let change = state.apply(&line);
            if change == Change::Position && first.is_none() {
                first = Some(state.position);
            }
            if change != Change::None && change != Change::Position {
                changes.push(change);
            }
        }
        let _ = child.wait().await;
        let _ = std::fs::remove_file(&socket);
        assert!(first.unwrap() >= 1.9, "started at {:?}", first);
        assert_eq!(changes, vec![Change::Ended]);
        let (position, duration) = state.saveable().unwrap();
        assert_eq!(position, duration);
        assert!(state.ended && duration > 11_000);
    }

    #[test]
    fn observers_cover_the_synced_properties() {
        let commands = observe_commands();
        assert_eq!(commands.lines().count(), 4);
        for line in commands.lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["command"][0], "observe_property");
        }
        assert!(commands.contains("\"time-pos\"") && commands.contains("\"eof-reached\""));
    }

    #[test]
    fn socket_lives_in_a_runtime_or_temp_directory() {
        let path = socket_path(42);
        assert!(path.is_absolute());
        assert_eq!(path.file_name().unwrap(), "anix-mpv-42.sock");
    }
}
