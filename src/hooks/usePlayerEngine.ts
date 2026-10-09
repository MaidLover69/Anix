import { useCallback, useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import Hls from 'hls.js';
import type { ErrorData, Level, MediaPlaylist } from 'hls.js';
import { buildProxyUrl } from '../lib/streamProxy';
import type { StreamItem } from '../types';

export type EnginePhase = 'loading' | 'ready' | 'error';

export interface QualityLevel {
  id: number;
  height: number;
  bitrate: number;
}

export interface MediaTrackOption {
  id: number;
  label: string;
}

export interface PlayerEngineOptions {
  videoRef: RefObject<HTMLVideoElement>;
  /** Tried in order; a fatal error on one moves on to the next. */
  streams: StreamItem[];
  proxyBase: string;
  /** Loading waits until this is true (the resume position must be known first). */
  enabled: boolean;
  /** Seconds to start from; 0 starts at the beginning. */
  startAt: number;
  /** False when `startAt` could not be checked against a known duration. */
  startVerified: boolean;
  autoplay: boolean;
}

export interface PlayerEngine {
  phase: EnginePhase;
  error: string | null;
  /** The stream currently loaded. */
  stream: StreamItem | null;
  /** True while hls.js drives the element (and with it, its text tracks). */
  usesHls: boolean;
  levels: QualityLevel[];
  /** The user's choice; -1 is automatic. */
  selectedLevel: number;
  /** The level hls.js is actually playing; -1 when unknown. */
  activeLevel: number;
  selectLevel: (id: number) => void;
  audioTracks: MediaTrackOption[];
  audioTrack: number;
  selectAudioTrack: (id: number) => void;
  /** Subtitle renditions inside the HLS manifest. */
  textTracks: MediaTrackOption[];
  textTrack: number;
  selectTextTrack: (id: number) => void;
  play: () => void;
  pause: () => void;
  togglePlay: () => void;
}

const MAX_NETWORK_RECOVERIES = 2;
const MAX_MEDIA_RECOVERIES = 2;
/** A saved position this close to the end means the episode was finished. */
const RESUME_END_FRACTION = 0.95;

function isHlsStream(stream: StreamItem): boolean {
  return (
    stream.stream_type?.toLowerCase() === 'hls' ||
    stream.url.includes('.m3u8') ||
    stream.url.includes('/hls/')
  );
}

function toLevels(levels: Level[]): QualityLevel[] {
  return levels.map((level, id) => ({ id, height: level.height || 0, bitrate: level.bitrate || 0 }));
}

function toTrackOptions(tracks: MediaPlaylist[]): MediaTrackOption[] {
  return tracks.map((track) => ({
    id: track.id,
    label: track.name || track.lang || `Track ${track.id + 1}`,
  }));
}

function describeHlsError(data: ErrorData): string {
  const status = data.response?.code;
  const suffix = ` (${data.details}${status ? `, HTTP ${status}` : ''})`;
  switch (data.details) {
    case Hls.ErrorDetails.MANIFEST_LOAD_ERROR:
    case Hls.ErrorDetails.MANIFEST_LOAD_TIMEOUT:
      return `The stream playlist could not be loaded${suffix}`;
    case Hls.ErrorDetails.MANIFEST_PARSING_ERROR:
      return `The stream playlist is not valid${suffix}`;
    case Hls.ErrorDetails.MANIFEST_INCOMPATIBLE_CODECS_ERROR:
      return `This stream uses a codec the player cannot decode${suffix}`;
    default:
      if (data.type === Hls.ErrorTypes.NETWORK_ERROR) return `The stream stopped loading${suffix}`;
      if (data.type === Hls.ErrorTypes.MEDIA_ERROR) return `The stream could not be decoded${suffix}`;
      return `Playback failed${suffix}`;
  }
}

function describeMediaError(error: MediaError | null): string {
  const detail = error?.message ? ` (${error.message})` : '';
  switch (error?.code) {
    case MediaError.MEDIA_ERR_NETWORK:
      return `The video stopped loading because of a network error${detail}`;
    case MediaError.MEDIA_ERR_DECODE:
      return `The video could not be decoded${detail}`;
    case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
      return `The video could not be loaded, or its format is not supported${detail}`;
    default:
      return `The video could not be played${detail}`;
  }
}

/** `play()` rejects with a DOMException; read it without assuming its prototype chain. */
function readRejection(reason: unknown): { name: string; message: string } {
  if (reason && typeof reason === 'object') {
    const { name, message } = reason as { name?: unknown; message?: unknown };
    return {
      name: typeof name === 'string' ? name : '',
      message: typeof message === 'string' ? message : '',
    };
  }
  return { name: '', message: typeof reason === 'string' ? reason : '' };
}

/**
 * Owns the `<video>` source: hls.js for HLS, the element itself for direct files, always
 * through the backend stream proxy. Recovers from fatal errors a bounded number of times,
 * then falls through to the next stream, then reports an error.
 */
export function usePlayerEngine({
  videoRef,
  streams,
  proxyBase,
  enabled,
  startAt,
  startVerified,
  autoplay,
}: PlayerEngineOptions): PlayerEngine {
  const [streamIndex, setStreamIndex] = useState(0);
  const [phase, setPhase] = useState<EnginePhase>('loading');
  const [error, setError] = useState<string | null>(null);
  const [usesHls, setUsesHls] = useState(false);
  const [levels, setLevels] = useState<QualityLevel[]>([]);
  const [selectedLevel, setSelectedLevel] = useState(-1);
  const [activeLevel, setActiveLevel] = useState(-1);
  const [audioTracks, setAudioTracks] = useState<MediaTrackOption[]>([]);
  const [audioTrack, setAudioTrack] = useState(-1);
  const [textTracks, setTextTracks] = useState<MediaTrackOption[]>([]);
  const [textTrack, setTextTrack] = useState(-1);

  const hlsRef = useRef<Hls | null>(null);
  // Where the next load starts: the saved position at first, the failure point after that.
  const resumeRef = useRef<{ time: number; verified: boolean } | null>(null);
  // Whether the user (or autoplay) wants playback, independent of what the element is doing.
  const wantPlayRef = useRef(autoplay);
  const firstErrorRef = useRef<string | null>(null);
  const playRejectedRef = useRef<(reason: unknown) => void>(() => {});

  useEffect(() => {
    const video = videoRef.current;
    const stream = streams[streamIndex];
    if (!enabled || !video || !stream) return;

    if (!resumeRef.current) resumeRef.current = { time: startAt, verified: startVerified };
    const start = resumeRef.current;
    const url = buildProxyUrl(proxyBase, stream);

    let disposed = false;
    let failed = false;
    let startApplied = false;
    let manifestParsed = false;
    let networkRecoveries = 0;
    let mediaRecoveries = 0;
    let lastMediaRecovery = 0;
    let hls: Hls | null = null;

    setPhase('loading');
    setError(null);
    setLevels([]);
    setSelectedLevel(-1);
    setActiveLevel(-1);
    setAudioTracks([]);
    setAudioTrack(-1);
    setTextTracks([]);
    setTextTrack(-1);

    const fail = (message: string) => {
      if (disposed || failed) return;
      failed = true;
      if (Number.isFinite(video.currentTime) && video.currentTime > 0) {
        resumeRef.current = { time: video.currentTime, verified: true };
      }
      if (!firstErrorRef.current) firstErrorRef.current = message;
      if (streamIndex < streams.length - 1) {
        setStreamIndex(streamIndex + 1);
        return;
      }
      hls?.destroy();
      hls = null;
      hlsRef.current = null;
      const tried = streams.length > 1 ? ` Tried ${streams.length} streams.` : '';
      setError(`${firstErrorRef.current}.${tried}`);
      setPhase('error');
    };

    const onPlayRejected = (reason: unknown) => {
      if (disposed) return;
      const { name, message } = readRejection(reason);
      // AbortError: a newer load or pause interrupted this play request.
      if (name === 'AbortError') return;
      // NotAllowedError: autoplay is blocked; stay paused until the user presses play.
      if (name === 'NotAllowedError') {
        wantPlayRef.current = false;
        return;
      }
      fail(`Playback could not start${message ? ` (${message})` : ''}`);
    };
    playRejectedRef.current = onPlayRejected;

    const recoverMedia = (): boolean => {
      if (!hls || mediaRecoveries >= MAX_MEDIA_RECOVERIES) return false;
      const now = Date.now();
      // hls.js and the element can both report the same decode failure.
      if (now - lastMediaRecovery < 1000) return true;
      lastMediaRecovery = now;
      mediaRecoveries += 1;
      if (mediaRecoveries > 1) hls.swapAudioCodec();
      hls.recoverMediaError();
      return true;
    };

    const onLoadedMetadata = () => {
      if (failed) return;
      if (!startApplied) {
        startApplied = true;
        if (start.time > 0) {
          const duration = video.duration;
          const finished =
            Number.isFinite(duration) && duration > 0 && start.time >= duration * RESUME_END_FRACTION;
          if (!finished && Math.abs(video.currentTime - start.time) > 1) {
            video.currentTime = start.time;
          }
        }
      }
      setPhase('ready');
      // Also after a media-error recovery, which reloads the element and leaves it paused.
      if (wantPlayRef.current) video.play().catch(onPlayRejected);
    };

    const onVideoError = () => {
      if (hls && video.error?.code === MediaError.MEDIA_ERR_DECODE && recoverMedia()) return;
      fail(describeMediaError(video.error));
    };

    video.addEventListener('loadedmetadata', onLoadedMetadata);
    video.addEventListener('error', onVideoError);

    const hlsStream = isHlsStream(stream);
    setUsesHls(hlsStream && Hls.isSupported());
    if (hlsStream && Hls.isSupported()) {
      const instance = new Hls({
        enableWorker: true,
        lowLatencyMode: false,
        maxBufferLength: 30,
        maxBufferSize: 60 * 1024 * 1024,
        backBufferLength: 90,
        enableCEA708Captions: false,
        startPosition: start.verified && start.time > 0 ? start.time : -1,
      });
      hls = instance;
      hlsRef.current = instance;

      instance.on(Hls.Events.MANIFEST_PARSED, (_event, data) => {
        manifestParsed = true;
        setLevels(toLevels(data.levels));
      });
      instance.on(Hls.Events.LEVEL_SWITCHED, (_event, data) => setActiveLevel(data.level));
      instance.on(Hls.Events.AUDIO_TRACKS_UPDATED, (_event, data) => {
        setAudioTracks(toTrackOptions(data.audioTracks));
        setAudioTrack(instance.audioTrack);
      });
      instance.on(Hls.Events.AUDIO_TRACK_SWITCHED, (_event, data) => setAudioTrack(data.id));
      instance.on(Hls.Events.SUBTITLE_TRACKS_UPDATED, (_event, data) =>
        setTextTracks(toTrackOptions(data.subtitleTracks))
      );
      instance.on(Hls.Events.SUBTITLE_TRACK_SWITCH, (_event, data) => setTextTrack(data.id));
      instance.on(Hls.Events.ERROR, (_event, data) => {
        if (!data.fatal || disposed || failed) return;
        if (data.type === Hls.ErrorTypes.MEDIA_ERROR && recoverMedia()) return;
        if (
          data.type === Hls.ErrorTypes.NETWORK_ERROR &&
          manifestParsed &&
          networkRecoveries < MAX_NETWORK_RECOVERIES
        ) {
          networkRecoveries += 1;
          instance.startLoad();
          return;
        }
        fail(describeHlsError(data));
      });

      instance.loadSource(url);
      instance.attachMedia(video);
    } else if (hlsStream && !video.canPlayType('application/vnd.apple.mpegurl')) {
      fail('This webview cannot play HLS streams (Media Source Extensions are unavailable)');
    } else {
      video.src = url;
    }

    return () => {
      disposed = true;
      video.removeEventListener('loadedmetadata', onLoadedMetadata);
      video.removeEventListener('error', onVideoError);
      hls?.destroy();
      hlsRef.current = null;
      // Without this a direct-file stream keeps playing under the next episode.
      video.pause();
      video.removeAttribute('src');
      video.load();
    };
    // `startAt`, `startVerified` and `autoplay` are read once, when loading first begins.
  }, [enabled, proxyBase, streams, streamIndex, videoRef]);

  const selectLevel = useCallback((id: number) => {
    const hls = hlsRef.current;
    if (!hls) return;
    setSelectedLevel(id);
    // Automatic: only affects what loads next. A fixed level replaces the buffer right away.
    if (id < 0) hls.loadLevel = -1;
    else hls.currentLevel = id;
  }, []);

  const selectAudioTrack = useCallback((id: number) => {
    const hls = hlsRef.current;
    if (hls) hls.audioTrack = id;
  }, []);

  const selectTextTrack = useCallback((id: number) => {
    const hls = hlsRef.current;
    if (!hls) return;
    hls.subtitleTrack = id;
    if (id >= 0) hls.subtitleDisplay = true;
  }, []);

  const play = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    wantPlayRef.current = true;
    video.play().catch((reason) => playRejectedRef.current(reason));
  }, [videoRef]);

  const pause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    wantPlayRef.current = false;
    video.pause();
  }, [videoRef]);

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused || video.ended) play();
    else pause();
  }, [videoRef, play, pause]);

  return {
    phase,
    error,
    stream: streams[streamIndex] ?? null,
    usesHls,
    levels,
    selectedLevel,
    activeLevel,
    selectLevel,
    audioTracks,
    audioTrack,
    selectAudioTrack,
    textTracks,
    textTrack,
    selectTextTrack,
    play,
    pause,
    togglePlay,
  };
}
