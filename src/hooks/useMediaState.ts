import { useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import { usePlayerStore } from '../stores/playerStore';
import { startRateFor } from '../lib/speed';

// The episode the last player showed, so a remount within it keeps the speed.
let lastEpisodeKey: string | null = null;

export interface MediaState {
  playing: boolean;
  buffering: boolean;
  ended: boolean;
  muted: boolean;
  volume: number;
  rate: number;
  /** Seconds; 0 until the media reports a finite duration. */
  duration: number;
}

/**
 * Player state read from the `<video>` element's own events, so the UI can never disagree
 * with the media. The current time is left out on purpose: it changes several times a
 * second and only the timeline needs it.
 */
export function useMediaState(videoRef: RefObject<HTMLVideoElement>, episodeKey: string, defaultSpeed?: unknown): MediaState {
  const startRate = useRef<number | null>(null);
  if (startRate.current === null) {
    startRate.current = startRateFor(episodeKey, lastEpisodeKey, usePlayerStore.getState().playbackRate, defaultSpeed);
  }
  const [state, setState] = useState<MediaState>(() => {
    const prefs = usePlayerStore.getState();
    return {
      playing: false,
      buffering: false,
      ended: false,
      muted: prefs.muted,
      volume: prefs.volume,
      rate: startRate.current ?? 1,
      duration: 0,
    };
  });

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // A new element starts at full volume and normal speed; carry the user's choices over.
    const prefs = usePlayerStore.getState();
    // Out-of-range values throw when assigned, and the volume comes from storage.
    video.volume = prefs.volume >= 0 && prefs.volume <= 1 ? prefs.volume : 1;
    video.muted = prefs.muted === true;
    // A new episode starts at the default speed; the same one keeps what the user chose.
    lastEpisodeKey = episodeKey;
    const rate = startRate.current ?? 1;
    usePlayerStore.getState().setPlaybackRate(rate);
    video.defaultPlaybackRate = rate;
    video.playbackRate = rate;

    const patch = (next: Partial<MediaState>) =>
      setState((prev) => {
        const keys = Object.keys(next) as (keyof MediaState)[];
        return keys.some((key) => prev[key] !== next[key]) ? { ...prev, ...next } : prev;
      });

    const readDuration = () =>
      Number.isFinite(video.duration) && video.duration > 0 ? video.duration : 0;

    const syncPlayback = () =>
      patch({
        playing: !video.paused && !video.ended && !video.error,
        ended: video.ended,
      });
    const onWaiting = () => patch({ buffering: true });
    const onFlowing = () => {
      patch({ buffering: false });
      syncPlayback();
    };
    const onStopped = () => {
      patch({ buffering: false });
      syncPlayback();
    };
    const onVolumeChange = () => {
      patch({ volume: video.volume, muted: video.muted });
      usePlayerStore.getState().setVolume(video.volume, video.muted);
    };
    const onRateChange = () => patch({ rate: video.playbackRate });
    const onDurationChange = () => patch({ duration: readDuration() });
    const onLoadedMetadata = () => {
      // Loading a source resets the rate; put the chosen speed back.
      const wanted = usePlayerStore.getState().playbackRate;
      if (video.playbackRate !== wanted) video.playbackRate = wanted;
      patch({ duration: readDuration() });
    };
    const onEmptied = () => patch({ playing: false, buffering: false, ended: false, duration: 0 });

    const listeners: [keyof HTMLMediaElementEventMap, () => void][] = [
      ['play', syncPlayback],
      ['playing', onFlowing],
      ['canplay', onFlowing],
      ['seeked', onFlowing],
      ['pause', onStopped],
      ['ended', onStopped],
      ['error', onStopped],
      ['waiting', onWaiting],
      ['volumechange', onVolumeChange],
      ['ratechange', onRateChange],
      ['durationchange', onDurationChange],
      ['loadedmetadata', onLoadedMetadata],
      ['emptied', onEmptied],
    ];
    listeners.forEach(([name, handler]) => video.addEventListener(name, handler));
    return () => listeners.forEach(([name, handler]) => video.removeEventListener(name, handler));
  }, [videoRef]);

  return state;
}
