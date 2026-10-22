import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { RefObject } from 'react';
import type { SkipTimes } from '../types';
import { countdownLeft, fallbackSkipTarget, SKIP_COUNTDOWN_SECONDS } from '../components/player/playerLogic';

export type SkipKind = 'intro' | 'outro';

export interface SkipSegment {
  kind: SkipKind;
  start: number;
  end: number;
}

/** True when the provider or AniSkip supplied at least one usable segment. */
export function hasSkipTimes(skip: SkipTimes | null | undefined): skip is SkipTimes {
  return toSegments(skip).length > 0;
}

function toSegments(skip: SkipTimes | null | undefined): SkipSegment[] {
  if (!skip) return [];
  const segments: SkipSegment[] = [];
  const add = (kind: SkipKind, start: number | null, end: number | null) => {
    if (start !== null && end !== null && Number.isFinite(start) && Number.isFinite(end) && end > start) {
      segments.push({ kind, start, end });
    }
  };
  add('intro', skip.intro_start, skip.intro_end);
  add('outro', skip.outro_start, skip.outro_end);
  return segments;
}

export interface AutoSkip {
  intro: boolean;
  outro: boolean;
}

/**
 * Tracks which intro/outro segment the playhead is in. A segment whose kind is switched on in
 * `auto` is jumped after a 3-second countdown on the button (Enter skips at once, cancelling
 * keeps playing); each one only once. Without skip times, `fallback` offers a fixed jump in
 * the first minutes (`lookupDone` keeps it away while the skip times are still being asked for).
 */
export function useSkipSegments(
  videoRef: RefObject<HTMLVideoElement>,
  skip: SkipTimes | null | undefined,
  auto: AutoSkip,
  lookupDone: boolean,
  onAutoSkip?: (kind: SkipKind) => void
) {
  const segments = useMemo(
    () => toSegments(skip),
    [skip?.intro_start, skip?.intro_end, skip?.outro_start, skip?.outro_end]
  );
  const [active, setActive] = useState<SkipSegment | null>(null);
  /** Seconds left of the countdown, or null when none runs. */
  const [countdown, setCountdown] = useState<number | null>(null);
  const [fallbackShown, setFallbackShown] = useState(false);
  const handled = useRef(new Set<SkipKind>());
  const fallbackUsed = useRef(false);
  // When the running countdown started (ms), moved forward while paused so pausing holds it.
  const countdownRef = useRef<{ kind: SkipKind; startedAt: number; pausedAt: number | null } | null>(null);
  const autoRef = useRef(auto);
  autoRef.current = auto;
  const onAutoSkipRef = useRef(onAutoSkip);
  onAutoSkipRef.current = onAutoSkip;
  const hasTimes = segments.length > 0;

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const stopCountdown = () => {
      countdownRef.current = null;
      setCountdown(null);
    };

    const check = () => {
      const time = video.currentTime;
      const segment = segments.find((s) => time >= s.start && time < s.end - 0.5) ?? null;
      setFallbackShown(!fallbackUsed.current && lookupDone && fallbackSkipTarget(time, video.duration, hasTimes) !== null);

      const running = countdownRef.current;
      if (running && running.kind !== segment?.kind) {
        // Left the segment (a seek, or it ended): it counts as dealt with.
        handled.current.add(running.kind);
        stopCountdown();
      }
      if (
        segment &&
        autoRef.current[segment.kind] &&
        !handled.current.has(segment.kind) &&
        !countdownRef.current &&
        !video.paused &&
        !video.seeking
      ) {
        countdownRef.current = { kind: segment.kind, startedAt: Date.now(), pausedAt: null };
        setCountdown(SKIP_COUNTDOWN_SECONDS);
      }

      setActive((prev) =>
        prev?.kind === segment?.kind && prev?.start === segment?.start && prev?.end === segment?.end
          ? prev
          : segment
      );
    };

    const tick = window.setInterval(() => {
      const running = countdownRef.current;
      if (!running) return;
      const now = Date.now();
      if (video.paused) {
        running.pausedAt ??= now;
        return;
      }
      if (running.pausedAt !== null) {
        running.startedAt += now - running.pausedAt;
        running.pausedAt = null;
      }
      const left = countdownLeft(running.startedAt, now);
      if (left > 0) {
        setCountdown(left);
        return;
      }
      const segment = segments.find((s) => s.kind === running.kind);
      handled.current.add(running.kind);
      stopCountdown();
      if (segment) {
        video.currentTime = segment.end;
        onAutoSkipRef.current?.(segment.kind);
        setActive(null);
      }
    }, 200);

    check();
    video.addEventListener('timeupdate', check);
    video.addEventListener('seeked', check);
    video.addEventListener('durationchange', check);
    return () => {
      window.clearInterval(tick);
      video.removeEventListener('timeupdate', check);
      video.removeEventListener('seeked', check);
      video.removeEventListener('durationchange', check);
    };
  }, [videoRef, segments, hasTimes, lookupDone]);

  const skipActive = useCallback(() => {
    const video = videoRef.current;
    if (!video || !active) return;
    handled.current.add(active.kind);
    countdownRef.current = null;
    setCountdown(null);
    video.currentTime = active.end;
    setActive(null);
  }, [videoRef, active]);

  /** Stops the countdown; the button stays for a manual skip. True when one was running. */
  const cancelCountdown = useCallback((): boolean => {
    const running = countdownRef.current;
    if (!running) return false;
    handled.current.add(running.kind);
    countdownRef.current = null;
    setCountdown(null);
    return true;
  }, []);

  const skipFallback = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    const target = fallbackSkipTarget(video.currentTime, video.duration, hasTimes);
    if (target !== null) video.currentTime = target;
    fallbackUsed.current = true;
    setFallbackShown(false);
  }, [videoRef, hasTimes]);

  return { active, countdown, skipActive, cancelCountdown, fallbackShown, skipFallback };
}
