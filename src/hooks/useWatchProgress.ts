import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';
import { useUpdateWatchProgress } from './useLibrary';

export interface WatchProgressTarget {
  anilistId: number;
  title: string;
  cover: string | null;
  episodeNumber: number;
  episodeTitle: string | null;
  /** The provider that actually served the stream. */
  provider: string;
  category: 'sub' | 'dub';
}

const SAVE_INTERVAL_MS = 10_000;

/**
 * Saves the watch position of the episode in `videoRef`: on an interval while playing, on
 * pause, at the end, and when the player goes away. Nothing is written before playback has
 * really advanced, so opening an episode never replaces its saved position with 0, and the
 * duration is only ever the media's own finite duration.
 */
export function useWatchProgress(videoRef: RefObject<HTMLVideoElement>, target: WatchProgressTarget) {
  const { mutate } = useUpdateWatchProgress();
  const targetRef = useRef(target);
  targetRef.current = target;

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // Last values read from the element; the final save runs after its source is gone.
    let position = 0;
    let duration = 0;
    let played = false;
    let lastSaved = -1;

    const read = () => {
      if (video.readyState === 0) return;
      if (Number.isFinite(video.duration) && video.duration > 0) duration = video.duration;
      position = video.currentTime;
      if (!video.paused && !video.seeking && video.currentTime > 0) played = true;
    };

    const save = () => {
      if (!played || duration <= 0) return;
      const clamped = Math.min(Math.max(0, position), duration);
      if (Math.abs(clamped - lastSaved) < 1) return;
      lastSaved = clamped;
      mutate({
        ...targetRef.current,
        positionMs: Math.round(clamped * 1000),
        durationMs: Math.round(duration * 1000),
      });
    };

    const onPause = () => {
      read();
      save();
    };
    const onEnded = () => {
      read();
      // A stream torn down part-way also fires `ended`; only a real end counts as finished.
      if (duration > 0 && position >= duration - 2) position = duration;
      save();
    };

    video.addEventListener('timeupdate', read);
    video.addEventListener('seeked', read);
    video.addEventListener('pause', onPause);
    video.addEventListener('ended', onEnded);
    const interval = window.setInterval(() => {
      if (!video.paused) save();
    }, SAVE_INTERVAL_MS);

    return () => {
      window.clearInterval(interval);
      video.removeEventListener('timeupdate', read);
      video.removeEventListener('seeked', read);
      video.removeEventListener('pause', onPause);
      video.removeEventListener('ended', onEnded);
      save();
    };
  }, [videoRef, mutate]);
}
