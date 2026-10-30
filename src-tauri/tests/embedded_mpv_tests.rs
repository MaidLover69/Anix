use anix_lib::embedded_mpv::{is_gpl_license, margins, plan, Action, AudioPreset, Fit, Op};

const BASE: &str = "http://127.0.0.1:40000/token";

fn action(json: &str) -> Action {
    serde_json::from_str(json).expect("valid action")
}

#[test]
fn loads_only_from_the_stream_proxy() {
    let ok = plan(&action(r#"{"action":"load","url":"http://127.0.0.1:40000/token/proxy?url=x","start":12.5}"#), BASE).unwrap();
    assert_eq!(ok[0], Op::Set("start", "12.500".into()));
    assert_eq!(ok[1], Op::Command(vec!["loadfile".into(), "http://127.0.0.1:40000/token/proxy?url=x".into(), "replace".into()]));
    for url in [
        "https://example.com/a.m3u8",
        "http://127.0.0.1:40000/other/proxy",
        "http://127.0.0.1:40000/token",
        "file:///etc/passwd",
        "http://127.0.0.1:40000/token/proxy?url=x\nquit",
    ] {
        let load = Action::Load { url: url.into(), start: None };
        assert!(plan(&load, BASE).is_err(), "{} must be refused", url);
    }
}

#[test]
fn start_defaults_to_none() {
    let ops = plan(&action(r#"{"action":"load","url":"http://127.0.0.1:40000/token/p","start":null}"#), BASE).unwrap();
    assert_eq!(ops[0], Op::Set("start", "none".into()));
}

#[test]
fn values_are_clamped_and_must_be_numbers() {
    assert_eq!(plan(&Action::SetVolume { value: 500.0 }, BASE).unwrap(), vec![Op::Set("volume", "130.0".into())]);
    assert_eq!(plan(&Action::SetSpeed { value: 0.0 }, BASE).unwrap(), vec![Op::Set("speed", "0.250".into())]);
    assert!(plan(&Action::Seek { seconds: f64::NAN, absolute: true }, BASE).is_err());
    assert!(plan(&Action::SetSubTrack { id: Some(0) }, BASE).is_err());
    assert_eq!(plan(&Action::SetSubTrack { id: None }, BASE).unwrap(), vec![Op::Set("sid", "no".into())]);
}

#[test]
fn unknown_actions_and_presets_are_rejected() {
    assert!(serde_json::from_str::<Action>(r#"{"action":"command","args":["quit"]}"#).is_err());
    assert!(serde_json::from_str::<Action>(r#"{"action":"set_audio_preset","preset":"lavfi=[x]"}"#).is_err());
    let ops = plan(&Action::SetAudioPreset { preset: AudioPreset::None }, BASE).unwrap();
    assert_eq!(ops, vec![Op::Set("af", String::new())]);
    let fill = plan(&Action::SetFit { fit: Fit::Fill }, BASE).unwrap();
    assert_eq!(fill[0], Op::Set("keepaspect", "no".into()));
}

#[test]
fn subtitle_titles_are_cleaned() {
    let ops = plan(
        &Action::AddSubtitle { url: format!("{}/proxy?url=s", BASE), title: Some("English\n".into()), select: true },
        BASE,
    )
    .unwrap();
    assert_eq!(ops[0], Op::CommandAsync(vec!["sub-add".into(), format!("{}/proxy?url=s", BASE), "cached".into(), "English".into()]));
}

#[test]
fn subtitle_files_are_added_without_blocking_playback() {
    // sub-add downloads the whole file before it returns; run synchronously it froze playback.
    let add = |select| plan(&Action::AddSubtitle { url: format!("{}/s", BASE), title: None, select }, BASE).unwrap();
    // "cached" shows the file and reuses it when it was added before, instead of a second copy.
    assert_eq!(add(true), vec![Op::CommandAsync(vec!["sub-add".into(), format!("{}/s", BASE), "cached".into(), String::new()])]);
    assert_eq!(add(false), vec![Op::CommandAsync(vec!["sub-add".into(), format!("{}/s", BASE), "auto".into(), String::new()])]);
    let outside = Action::AddSubtitle { url: "https://example.com/s.ass".into(), title: None, select: true };
    assert!(plan(&outside, BASE).is_err());
}

#[test]
fn loaded_event_carries_mpv_pause_state() {
    use anix_lib::embedded_mpv::loaded_event;
    use serde_json::json;
    // mpv only reports `pause` when it changes, so the page learns the state at load.
    assert_eq!(loaded_event(json!([]), Some("no")), json!({ "kind": "loaded", "tracks": [], "paused": false }));
    assert_eq!(loaded_event(json!([]), Some("yes"))["paused"], json!(true));
    assert_eq!(loaded_event(json!([]), None)["paused"], json!(null));
}

#[test]
fn video_rect_becomes_margin_ratios() {
    let [l, r, t, b] = margins(0.0, 40.0, 1000.0, 560.0, 1000.0, 600.0).unwrap();
    assert_eq!((l, r), (0.0, 0.0));
    assert!((t - 40.0 / 600.0).abs() < 1e-9 && b.abs() < 1e-9);
    assert!(margins(0.0, 0.0, 1.0, 1.0, 1000.0, 600.0).is_err());
    assert!(margins(0.0, 0.0, 10.0, 10.0, 0.0, 600.0).is_err());
}

#[test]
fn recognises_gpl_ffmpeg_builds() {
    assert!(is_gpl_license("GPL version 2 or later"));
    assert!(is_gpl_license("nonfree and unredistributable"));
    assert!(!is_gpl_license("LGPL version 2.1 or later"));
}

// ---- engine choice, confirm-or-revert, hwdec, self-test, black watch, cache, Anime4K ------

use anix_lib::embedded_mpv::policy::{
    self, all_black, anime4k_files, cache_options, colours_match, decide, forced_web, hwdec_value, is_interop,
    needs_confirmation, selftest_due, shader_list, valid_route, Anime4k, Answer, BlackWatch, EngineState, HwdecMode,
    SelfTest, Start,
};
use serde_json::json;
use std::path::Path;

#[test]
fn engine_decision_follows_setting_flags_and_unanswered_trials() {
    let fresh = EngineState::default();
    assert_eq!(decide("web", None, &fresh), Start::Web { reason: None });
    assert_eq!(decide("external", None, &fresh), Start::Web { reason: None });
    assert_eq!(decide("mpv", None, &fresh), Start::Mpv);
    let forced = forced_web(&["anix".into(), "--web-player".into()], None);
    assert!(matches!(decide("mpv", forced, &fresh), Start::Web { reason: Some(_) }));
    assert!(forced_web(&[], Some(" WEB ")).is_some());
    assert!(forced_web(&[], Some("mpv")).is_none());
    // Killed during the question: the next start is on the web engine, with the reason.
    let killed = EngineState { trial: true, ..Default::default() };
    assert_eq!(decide("mpv", None, &killed), Start::Web { reason: Some(policy::UNCONFIRMED_REASON.to_string()) });
}

#[test]
fn confirmation_is_asked_once_per_switch_and_per_libmpv_or_webkit_version() {
    let fp = policy::fingerprint("mpv v0.41.0", "2.50.1");
    let mut state = EngineState { trial: true, ..Default::default() };
    assert!(needs_confirmation(&state, &fp));
    state = policy::answer(state, Answer::Keep, &fp);
    assert!(!state.trial && !needs_confirmation(&state, &fp));
    // A WebKit update asks again.
    assert!(needs_confirmation(&state, &policy::fingerprint("mpv v0.41.0", "2.52.0")));
    // Switching back in Settings and to mpv again asks again.
    state = policy::engine_changed(state, "web");
    assert!(!needs_confirmation(&state, &fp));
    state = policy::engine_changed(state, "mpv");
    assert!(needs_confirmation(&state, &fp));
    let reverted = policy::answer(EngineState { trial: true, ..Default::default() }, Answer::NoAnswer, &fp);
    assert!(!reverted.trial && reverted.confirmed.is_none() && reverted.reason.is_some());
}

#[test]
fn only_in_app_routes_are_reopened() {
    assert!(valid_route("/watch/21/3?cat=sub&prov=auto"));
    assert!(valid_route("/settings"));
    for bad in ["https://example.com", "//example.com", "watch/1", "/a b", "/a\\b", ""] {
        assert!(!valid_route(bad), "{bad}");
    }
}

#[test]
fn hwdec_mode_uses_copy_back_unless_the_self_test_passed() {
    assert_eq!(hwdec_value(HwdecMode::Auto, false), "auto-copy-safe");
    assert_eq!(hwdec_value(HwdecMode::Auto, true), "auto-safe");
    assert_eq!(hwdec_value(HwdecMode::Copy, true), "auto-copy-safe");
    assert_eq!(hwdec_value(HwdecMode::Off, true), "no");
    assert_eq!(HwdecMode::from_setting(None), HwdecMode::Auto);
    assert_eq!(HwdecMode::from_setting(Some("off")), HwdecMode::Off);
    assert!(is_interop("vaapi") && !is_interop("vaapi-copy") && !is_interop("no") && !is_interop(""));
}

#[test]
fn self_test_runs_once_per_driver_mpv_and_app_version() {
    let stored = SelfTest { ok: true, driver: "Intel | Mesa | 4.6".into(), mpv: "mpv v0.41.0".into(), app: "1.3.0".into(), at: 1 };
    let state = EngineState { selftest: Some(stored), ..Default::default() };
    assert!(!selftest_due(&state, HwdecMode::Auto, "Intel | Mesa | 4.6", "mpv v0.41.0", "1.3.0"));
    assert!(selftest_due(&state, HwdecMode::Auto, "AMD | Mesa | 4.6", "mpv v0.41.0", "1.3.0"));
    assert!(selftest_due(&state, HwdecMode::Auto, "Intel | Mesa | 4.6", "mpv v0.41.0", "1.3.1"));
    assert!(!selftest_due(&state, HwdecMode::Copy, "AMD", "x", "y"));
    let recheck = EngineState { selftest_recheck: true, ..state };
    assert!(selftest_due(&recheck, HwdecMode::Auto, "Intel | Mesa | 4.6", "mpv v0.41.0", "1.3.0"));
}

#[test]
fn self_test_pixels_must_show_the_four_bands() {
    let good = [[250, 4, 3], [2, 240, 10], [5, 3, 251], [247, 250, 252]];
    assert!(colours_match(&good, &policy::SELFTEST_COLOURS, 60));
    assert!(!colours_match(&[[0, 0, 0]; 4], &policy::SELFTEST_COLOURS, 60));
    assert!(!colours_match(&good[..3], &policy::SELFTEST_COLOURS, 60));
    assert!(all_black(&[[0, 0, 0], [16, 12, 3]]) && !all_black(&[[0, 0, 0], [40, 0, 0]]) && !all_black(&[]));
}

#[test]
fn black_watch_switches_after_three_seconds_of_black_while_playing() {
    let mut watch = BlackWatch::default();
    assert!(!watch.observe(0.0, true, 10.0));
    assert!(!watch.observe(2.5, true, 12.5));
    assert!(watch.observe(3.0, true, 13.0));
    assert!(!watch.observe(3.5, true, 13.5), "fires only once");

    // A picture in between, or a paused playhead, does not count.
    let mut watch = BlackWatch::default();
    watch.observe(0.0, true, 10.0);
    watch.observe(2.0, false, 12.0);
    assert!(!watch.observe(4.0, true, 14.0));
    let mut paused = BlackWatch::default();
    paused.observe(0.0, true, 10.0);
    assert!(!paused.observe(5.0, true, 10.2));
}

#[test]
fn cache_settings_map_to_mpv_options() {
    let dir = Path::new("/var/cache/anix/video-cache");
    let defaults = cache_options(&json!({}), dir);
    assert!(defaults.contains(&("demuxer-max-bytes", "150MiB".to_string())));
    assert!(defaults.contains(&("cache-on-disk", "no".to_string())));
    assert!(!defaults.iter().any(|(k, _)| *k == "cache-dir"));
    let disk = cache_options(
        &json!({"player_buffer_mb": 64, "player_readahead_secs": 9999, "player_disk_cache": true, "player_disk_cache_mb": 2048}),
        dir,
    );
    assert!(disk.contains(&("demuxer-max-bytes", "2048MiB".to_string())));
    assert!(disk.contains(&("demuxer-readahead-secs", "600".to_string())));
    assert!(disk.contains(&("cache-dir", "/var/cache/anix/video-cache".to_string())));
}

#[test]
fn anime4k_presets_build_the_documented_chains() {
    assert!(anime4k_files(Anime4k::Off).is_empty());
    assert_eq!(
        anime4k_files(Anime4k::AFast),
        ["Clamp_Highlights", "Restore_CNN_M", "Upscale_CNN_x2_M", "AutoDownscalePre_x2", "AutoDownscalePre_x4", "Upscale_CNN_x2_S"]
    );
    assert_eq!(anime4k_files(Anime4k::CHq)[1], "Upscale_Denoise_CNN_x2_VL");
    let list = shader_list(Path::new("/opt/anix/resources/anime4k/"), Anime4k::BFast).unwrap();
    assert!(list.starts_with("/opt/anix/resources/anime4k/Anime4K_Clamp_Highlights.glsl:"));
    assert_eq!(shader_list(Path::new("/x"), Anime4k::Off).unwrap(), "");
    assert!(shader_list(Path::new("/a:b"), Anime4k::AHq).is_err());
    // Every file a preset names is bundled.
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/anime4k");
    for preset in [Anime4k::AFast, Anime4k::BFast, Anime4k::CFast, Anime4k::AHq, Anime4k::BHq, Anime4k::CHq] {
        for name in anime4k_files(preset) {
            assert!(bundled.join(format!("Anime4K_{name}.glsl")).is_file(), "{name} is missing");
        }
    }
    assert!(serde_json::from_str::<Action>(r#"{"action":"set_shaders","preset":"b_hq"}"#).is_ok());
}

#[test]
fn second_subtitle_track_and_position() {
    assert_eq!(plan(&Action::SetSecondarySubTrack { id: Some(3) }, BASE).unwrap(), vec![Op::Set("secondary-sid", "3".into())]);
    assert_eq!(plan(&Action::SetSecondarySubPos { value: 12.4 }, BASE).unwrap(), vec![Op::Set("secondary-sub-pos", "12".into())]);
    assert_eq!(plan(&Action::SetShaders { preset: Anime4k::Off }, BASE).unwrap(), vec![Op::Shaders(Anime4k::Off)]);
}

#[test]
fn a_restart_drops_the_dmabuf_switch_anix_set_itself() {
    use anix_lib::system::hwaccel::{dmabuf_env, DmabufEnv};
    // Fresh start, web player: Anix turns the renderer off and marks the value as its own.
    assert_eq!(dmabuf_env(None, None, false), DmabufEnv::Disable);
    // Fresh start, mpv: the renderer stays on.
    assert_eq!(dmabuf_env(None, None, true), DmabufEnv::Untouched);
    // "Restart now" (web → mpv) inherits Anix's own value: it is removed again.
    assert_eq!(dmabuf_env(Some("1"), Some("1"), true), DmabufEnv::Clear);
    // A restart that stays on the web player keeps it off, still as Anix's own.
    assert_eq!(dmabuf_env(Some("1"), Some("1"), false), DmabufEnv::Disable);
    // A value the user set is never touched, with or without a stray mark.
    assert_eq!(dmabuf_env(Some("1"), None, true), DmabufEnv::User);
    assert_eq!(dmabuf_env(Some("0"), Some("1"), false), DmabufEnv::User);
    assert_eq!(dmabuf_env(Some("0"), None, true), DmabufEnv::User);
}

#[test]
fn a_web_engine_crash_during_web_playback_switches_to_mpv_only_once_and_only_when_mpv_can_run() {
    use policy::switch_to_mpv_after_crash as switch;
    let fresh = EngineState::default();
    assert!(switch("web", true, false, true, &fresh));
    // Not while nothing played on the web player, without a usable libmpv (RPM/DEB), or when
    // the engine already is mpv.
    assert!(!switch("web", false, false, true, &fresh));
    assert!(!switch("web", true, false, false, &fresh));
    assert!(!switch("mpv", true, false, true, &fresh));
    // Never twice: once mpv failed and the web player came back, it stays.
    let switched = policy::auto_switched_to_mpv(fresh);
    assert!(switched.auto_switched);
    assert!(!switch("web", true, false, true, &switched));
    // The switch is confirmed like any other (the window may be blank with mpv).
    let confirmed = EngineState { confirmed: Some("fp".into()), reason: Some("old".into()), ..Default::default() };
    let switched = policy::auto_switched_to_mpv(confirmed);
    assert!(switched.confirmed.is_none() && switched.reason.is_none());
}

#[test]
fn engine_state_files_from_before_the_crash_switch_still_load() {
    let old: EngineState = serde_json::from_value(json!({ "confirmed": "fp", "trial": false })).unwrap();
    assert!(!old.auto_switched);
    assert_eq!(old.confirmed.as_deref(), Some("fp"));
}

#[test]
fn a_web_playback_marker_counts_only_when_its_owner_is_gone() {
    use policy::marker_owner_gone as gone;
    const ME: u32 = 500;
    let alive = |pid: u32| pid == 42 || pid == ME;
    // A second launch while Anix plays: the marker belongs to the running instance, keep it.
    assert!(!gone("42", ME, alive));
    assert!(!gone("42\n", ME, alive));
    // Its owner died with the web player playing: a crash.
    assert!(gone("43", ME, alive));
    // An empty or garbled marker (an older build or a torn write) belongs to nobody alive.
    assert!(gone("", ME, alive));
    assert!(gone("not a pid", ME, alive));
    // A pid reused by this very start cannot be the owner: this start wrote nothing yet.
    assert!(gone("500", ME, alive));
}

#[test]
fn a_crash_does_not_switch_when_the_web_player_is_forced() {
    use policy::switch_to_mpv_after_crash as switch;
    let fresh = EngineState::default();
    assert!(switch("web", true, false, true, &fresh));
    // `--web-player` or ANIX_PLAYER_ENGINE=web: the user asked for the web player.
    assert!(!switch("web", true, true, true, &fresh));
}

#[test]
fn only_a_user_value_that_turns_the_dmabuf_renderer_off_blocks_mpv() {
    use anix_lib::system::hwaccel::dmabuf_off_by_user as off;
    assert!(off(Some("1"), None));
    assert!(off(Some("yes"), None));
    // "0" keeps the renderer on, which embedded mpv needs (install() accepts it too).
    assert!(!off(Some("0"), None));
    // Unset, or Anix's own value from an earlier run.
    assert!(!off(None, None));
    assert!(!off(Some("1"), Some("1")));
}
