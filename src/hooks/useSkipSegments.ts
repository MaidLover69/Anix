import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { RefObject } from 'react';
import type { SkipTimes } from '../types';

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

/**
 * Tracks which intro/outro segment the playhead is in. With `autoSkip` each segment is
 * jumped once; after that (for example when the user seeks back into it) only the manual
 * skip button is offered.
 */
export function useSkipSegments(
  videoRef: RefObject<HTMLVideoElement>,
  skip: SkipTimes | null | undefined,
  autoSkip: boolean,
  onAutoSkip?: (kind: SkipKind) => void
) {
  const segments = useMemo(
    () => toSegments(skip),
    [skip?.intro_start, skip?.intro_end, skip?.outro_start, skip?.outro_end]
  );
  const [active, setActive] = useState<SkipSegment | null>(null);
  const autoSkipped = useRef(new Set<SkipKind>());
  const autoSkipRef = useRef(autoSkip);
  autoSkipRef.current = autoSkip;
  const onAutoSkipRef = useRef(onAutoSkip);
  onAutoSkipRef.current = onAutoSkip;

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const check = () => {
      const time = video.currentTime;
      const segment = segments.find((s) => time >= s.start && time < s.end - 0.5) ?? null;

      if (
        segment &&
        autoSkipRef.current &&
        !autoSkipped.current.has(segment.kind) &&
        !video.paused &&
        !video.seeking
      ) {
        autoSkipped.current.add(segment.kind);
        video.currentTime = segment.end;
        onAutoSkipRef.current?.(segment.kind);
        setActive(null);
        return;
      }

      setActive((prev) =>
        prev?.kind === segment?.kind && prev?.start === segment?.start && prev?.end === segment?.end
          ? prev
          : segment
      );
    };

    check();
    video.addEventListener('timeupdate', check);
    video.addEventListener('seeked', check);
    return () => {
      video.removeEventListener('timeupdate', check);
      video.removeEventListener('seeked', check);
    };
  }, [videoRef, segments]);

  const skipActive = useCallback(() => {
    const video = videoRef.current;
    if (!video || !active) return;
    video.currentTime = active.end;
    setActive(null);
  }, [videoRef, active]);

  return { active, skipActive };
}
