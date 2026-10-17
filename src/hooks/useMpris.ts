import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface MprisTrack {
  /** "Anime – Episode N". */
  title: string;
  album: string;
  artUrl: string | null;
  /** Unique per episode. */
  track: string;
  canNext: boolean;
  canPrevious: boolean;
}

export interface MprisHandlers {
  play: () => void;
  pause: () => void;
  togglePlay: () => void;
  next: () => void;
  previous: () => void;
  seekTo: (seconds: number) => void;
  setVolume: (volume: number) => void;
  setRate: (rate: number) => void;
}

type Control =
  | { action: 'play' | 'pause' | 'playPause' | 'stop' | 'next' | 'previous' }
  | { action: 'seek'; offset: number }
  | { action: 'setPosition'; position: number }
  | { action: 'setVolume'; volume: number }
  | { action: 'setRate'; rate: number };

// The player is remounted for every episode. Withdrawing it from the bus is delayed a moment,
// so moving to the next episode does not make desktop widgets drop and re-add it.
const CLEAR_DELAY_MS = 1500;
let clearTimer: number | undefined;

/**
 * Publishes the player in `videoRef` over MPRIS (media keys, desktop widgets, `playerctl`)
 * while it is mounted, and carries out the requests that come back.
 */
export function useMpris(videoRef: RefObject<HTMLVideoElement>, track: MprisTrack, handlers: MprisHandlers) {
  const trackRef = useRef(track);
  trackRef.current = track;
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;
  const reportRef = useRef<(seeked?: boolean) => void>(() => {});

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    window.clearTimeout(clearTimer);

    let failed = false;
    const report = (seeked = false) => {
      if (failed) return;
      const t = trackRef.current;
      const duration = Number.isFinite(video.duration) && video.duration > 0 ? video.duration : 0;
      invoke('mpris_update', {
        state: {
          title: t.title,
          album: t.album,
          artUrl: t.artUrl,
          track: t.track,
          duration,
          position: video.currentTime || 0,
          playing: !video.paused && !video.ended,
          rate: video.playbackRate || 1,
          volume: video.muted ? 0 : video.volume,
          canNext: t.canNext,
          canPrevious: t.canPrevious,
          seeked,
        },
      }).catch(() => {
        // No session bus (or no D-Bus at all): the player simply is not published.
        failed = true;
      });
    };
    reportRef.current = report;

    const onSeeked = () => report(true);
    const onChange = () => report(false);
    const events = ['play', 'pause', 'ended', 'ratechange', 'durationchange', 'volumechange', 'loadedmetadata'];
    events.forEach((name) => video.addEventListener(name, onChange));
    video.addEventListener('seeked', onSeeked);
    report();

    let unlisten: (() => void) | null = null;
    let disposed = false;
    listen<Control>('mpris-control', ({ payload }) => {
      const h = handlersRef.current;
      switch (payload.action) {
        case 'play':
          h.play();
          break;
        case 'pause':
        case 'stop':
          h.pause();
          break;
        case 'playPause':
          h.togglePlay();
          break;
        case 'next':
          h.next();
          break;
        case 'previous':
          h.previous();
          break;
        case 'seek':
          h.seekTo((video.currentTime || 0) + payload.offset);
          break;
        case 'setPosition':
          h.seekTo(payload.position);
          break;
        case 'setVolume':
          h.setVolume(payload.volume);
          break;
        case 'setRate':
          h.setRate(payload.rate);
          break;
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
      events.forEach((name) => video.removeEventListener(name, onChange));
      video.removeEventListener('seeked', onSeeked);
      reportRef.current = () => {};
      window.clearTimeout(clearTimer);
      clearTimer = window.setTimeout(() => {
        invoke('mpris_clear').catch(() => {});
      }, CLEAR_DELAY_MS);
    };
  }, [videoRef]);

  // Title, cover or the next/previous buttons changed.
  useEffect(() => {
    reportRef.current(false);
  }, [track.title, track.album, track.artUrl, track.track, track.canNext, track.canPrevious]);
}
