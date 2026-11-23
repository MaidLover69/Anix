//! Fast drawing on NVIDIA (Settings › System) and the renderer variables Anix sets at start.

use anix_lib::system::fast_drawing::{answer, at_start, needs_question, FastDrawingState, NOT_CONFIRMED, SAFE_STARTED};
use anix_lib::system::hwaccel::{force_env, renderer_plan, ForceEnv, Gpu};

const GPU: Gpu = Gpu { nvidia: false, gles: true };
const NVIDIA: Gpu = Gpu { nvidia: true, gles: true };

#[test]
fn only_nvidia_without_mpv_or_opt_in_turns_the_renderer_off() {
    // (keep the renderer on, force it on against WebKit's own NVIDIA check)
    assert_eq!(renderer_plan(GPU, false, false), (true, false));
    assert_eq!(renderer_plan(GPU, true, true), (true, false));
    assert_eq!(renderer_plan(NVIDIA, false, false), (false, false));
    // Embedded mpv needs the renderer, and distro WebKit builds turn it off on NVIDIA unless forced.
    assert_eq!(renderer_plan(NVIDIA, true, false), (true, true));
    // The user opted in to fast drawing.
    assert_eq!(renderer_plan(NVIDIA, false, true), (true, true));
}

#[test]
fn without_opengl_es_the_renderer_stays_off_even_for_mpv() {
    // WebKit's web process stops at start when libGLESv2.so.2 cannot be loaded (seen on Debian 12 without libgles2).
    let no_gles = Gpu { gles: false, ..GPU };
    assert_eq!(renderer_plan(no_gles, false, false), (false, false));
    assert_eq!(renderer_plan(no_gles, true, true), (false, false));
    assert_eq!(renderer_plan(no_gles, true, false), (false, false));
    assert_eq!(renderer_plan(no_gles, false, true), (false, false));
    assert_eq!(renderer_plan(Gpu { gles: false, ..NVIDIA }, false, false), (false, false));
    assert_eq!(renderer_plan(Gpu { gles: false, ..NVIDIA }, true, true), (false, false));
}

#[test]
fn the_force_switch_is_set_and_taken_back_only_when_it_is_anixs() {
    assert_eq!(force_env(None, None, true), ForceEnv::Set);
    assert_eq!(force_env(None, None, false), ForceEnv::Untouched);
    // Set by an earlier run of Anix (a restart inherits the environment).
    assert_eq!(force_env(Some("1"), Some("1"), true), ForceEnv::Untouched);
    assert_eq!(force_env(Some("1"), Some("1"), false), ForceEnv::Clear);
    // The user's own value, either way, is never changed.
    assert_eq!(force_env(Some("1"), None, false), ForceEnv::User);
    assert_eq!(force_env(Some("0"), None, true), ForceEnv::User);
}

fn on() -> FastDrawingState {
    FastDrawingState { on: true, ..Default::default() }
}

#[test]
fn an_unanswered_question_turns_fast_drawing_off_at_the_next_start() {
    let state = FastDrawingState { trial: true, ..on() };
    let (next, use_fast) = at_start(state, false, false);
    assert!(!use_fast);
    assert!(!next.on && !next.trial);
    assert_eq!(next.reason.as_deref(), Some(NOT_CONFIRMED));
}

#[test]
fn safe_graphics_turns_it_off_and_safe_mode_only_skips_it() {
    let (next, use_fast) = at_start(on(), true, false);
    assert!(!use_fast && !next.on);
    assert_eq!(next.reason.as_deref(), Some(SAFE_STARTED));
    // Safe mode after crashes: not used this run, the choice is kept.
    let (next, use_fast) = at_start(on(), false, true);
    assert!(!use_fast && next.on);
    let (next, use_fast) = at_start(on(), false, false);
    assert!(use_fast && next == on());
}

#[test]
fn the_question_is_asked_once_per_webkit_version_and_not_over_embedded_mpv() {
    assert!(needs_question(&on(), true, false, "2.50.4"));
    // Embedded mpv asks its own question about the same window.
    assert!(!needs_question(&on(), true, true, "2.50.4"));
    assert!(!needs_question(&on(), false, false, "2.50.4"));
    let kept = answer(FastDrawingState { trial: true, ..on() }, true, "2.50.4");
    assert!(kept.on && !kept.trial);
    assert!(!needs_question(&kept, true, false, "2.50.4"));
    // A WebKit update asks again.
    assert!(needs_question(&kept, true, false, "2.52.1"));
    let declined = answer(FastDrawingState { trial: true, ..on() }, false, "2.50.4");
    assert!(!declined.on && !declined.trial && declined.confirmed.is_none());
}

#[test]
fn safe_mode_plan_turns_dmabuf_off_without_forcing_and_blames_itself_only_where_normal_keeps_it_on() {
    use anix_lib::system::hwaccel::plan;
    let no_gles = Gpu { gles: false, ..GPU };
    // Safe mode: off, never forced; "by safe mode" only if a normal start would keep it on.
    assert_eq!(plan(true, GPU, false, false), (false, false, true));
    assert_eq!(plan(true, GPU, true, false), (false, false, true));
    assert_eq!(plan(true, NVIDIA, true, false), (false, false, true));
    assert_eq!(plan(true, NVIDIA, false, true), (false, false, true));
    assert_eq!(plan(true, NVIDIA, false, false), (false, false, false));
    assert_eq!(plan(true, no_gles, false, false), (false, false, false));
    // Normal start: exactly renderer_plan, never "by safe mode".
    for gpu in [GPU, NVIDIA, no_gles] {
        for (mpv, fast) in [(false, false), (false, true), (true, false), (true, true)] {
            let (keep, force) = renderer_plan(gpu, mpv, fast);
            assert_eq!(plan(false, gpu, mpv, fast), (keep, force, false));
        }
    }
}
