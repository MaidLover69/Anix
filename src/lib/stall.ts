/** The player's stall watchdog: when playback that should be moving counts as stuck (S4). */

/** Playback that wants to run but has not moved this long counts as stuck. */
export const STALL_MS = 10_000;
/** A stream still loading gets longer: the first frame takes 8–11 s on slow sites. */
export const LOAD_STALL_MS = 20_000;
export interface StallSample {
  now: number;
  /** Current position, seconds. */
  time: number;
  phase: 'loading' | 'ready' | 'error';
  /** The user (or autoplay) asked to play and has not paused since. */
  wantsPlay: boolean;
  paused: boolean;
  /** The player paused itself to refill its buffer. */
  rebuffering: boolean;
}
export interface StallWatch {
  phase: StallSample['phase'];
  time: number;
  since: number;
}
/**
 * One tick of the stall watchdog: the next watch state, and whether playback is stuck. Only
 * loading, or playing that the user wants, counts; a pause by the user never does. A position
 * change or a new phase starts the clock again.
 */
export function watchStall(prev: StallWatch | null, s: StallSample): { watch: StallWatch; stuck: boolean } {
  const expecting = s.phase === 'loading' || (s.phase === 'ready' && s.wantsPlay && (!s.paused || s.rebuffering));
  const fresh = { phase: s.phase, time: s.time, since: s.now };
  if (!prev || !expecting || prev.phase !== s.phase || Math.abs(s.time - prev.time) > 0.05) {
    return { watch: fresh, stuck: false };
  }
  return { watch: prev, stuck: s.now - prev.since >= (s.phase === 'loading' ? LOAD_STALL_MS : STALL_MS) };
}
