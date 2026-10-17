import { useCallback, useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import Hls from 'hls.js';
import type { ErrorData, Level, MediaPlaylist } from 'hls.js';
import { normalizeLanguage } from '../lib/languages';
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
  /**
   * Audio languages to start on when the stream carries several (ISO 639-1, most wanted
   * first); the stream's own default is kept when none of them is offered.
   */
  preferredAudioLanguages?: string[];
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
  /** Playback is held after a stall until enough video is buffered. */
  rebuffering: boolean;
}

const MAX_NETWORK_RECOVERIES = 2;
/** Seconds of video (at 1x) to have ahead before playback continues after a stall. */
const REBUFFER_SECONDS = 4;
const REBUFFER_MAX_SECONDS = 20;
/** How far hls.js reads ahead at 1x; scaled up with the playback speed. */
const HLS_BUFFER_SECONDS = 30;
const HLS_BUFFER_BYTES = 60 * 1024 * 1024;
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

/** Index of the first track in a wanted language, trying the languages in order. */
export function preferredAudioTrack(tracks: Pick<MediaPlaylist, 'lang' | 'name'>[], wanted: string[]): number {
  const codes = tracks.map((track) => normalizeLanguage(track.lang) ?? normalizeLanguage(track.name));
  for (const language of wanted) {
    const index = codes.indexOf(language.toLowerCase());
    if (index !== -1) return index;
  }
  return -1;
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
  preferredAudioLanguages = [],
}: PlayerEngineOptions): PlayerEngine {
  const preferredAudioRef = useRef(preferredAudioLanguages);
  preferredAudioRef.current = preferredAudioLanguages;
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
  const [rebuffering, setRebuffering] = useState(false);

  const hlsRef = useRef<Hls | null>(null);
  // Set while playback is held after running out of data; see the rebuffer effect.
  const holdingRef = useRef(false);
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
        maxBufferLength: HLS_BUFFER_SECONDS,
        maxBufferSize: HLS_BUFFER_BYTES,
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
      let audioChosen = false;
      instance.on(Hls.Events.AUDIO_TRACKS_UPDATED, (_event, data) => {
        setAudioTracks(toTrackOptions(data.audioTracks));
        // Several audio languages, often none marked default: start on the wanted one, once.
        if (!audioChosen && data.audioTracks.length > 1) {
          audioChosen = true;
          const index = preferredAudioTrack(data.audioTracks, preferredAudioRef.current);
          if (index !== -1 && index !== instance.audioTrack) instance.audioTrack = index;
        }
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

  // After a stall, wait for a cushion of video before going on, instead of playing each
  // second as it arrives. The cushion and how far hls.js reads ahead grow with the speed.
  useEffect(() => {
    const video = videoRef.current;
    if (!enabled || !video) return;
    let timer: number | undefined;

    const bufferedAhead = () => {
      const t = video.currentTime;
      for (let i = 0; i < video.buffered.length; i++) {
        if (video.buffered.start(i) <= t + 0.25 && video.buffered.end(i) > t) return video.buffered.end(i) - t;
      }
      return 0;
    };
    const goal = () => {
      const rate = Math.max(1, video.playbackRate || 1);
      const remaining = Number.isFinite(video.duration) ? video.duration - video.currentTime - 0.5 : Infinity;
      return Math.max(0, Math.min(REBUFFER_SECONDS * rate, REBUFFER_MAX_SECONDS, remaining));
    };
    const release = (resume: boolean) => {
      window.clearInterval(timer);
      timer = undefined;
      if (!holdingRef.current) return;
      holdingRef.current = false;
      setRebuffering(false);
      if (resume && wantPlayRef.current) video.play().catch((reason) => playRejectedRef.current(reason));
    };
    const check = () => {
      if (!holdingRef.current) {
        window.clearInterval(timer);
        timer = undefined;
        return;
      }
      if (bufferedAhead() >= goal()) release(true);
    };
    const onWaiting = () => {
      // Seeks and the first start keep the element's own behaviour.
      if (holdingRef.current || video.seeking || video.currentTime < 1 || !wantPlayRef.current) return;
      if (bufferedAhead() >= goal()) return;
      holdingRef.current = true;
      setRebuffering(true);
      video.pause();
      timer = window.setInterval(check, 250);
    };
    const onSeeking = () => release(false);
    const onPlay = () => {
      // Played by the user (or the element) while held: stop holding.
      if (holdingRef.current) release(false);
    };
    const tuneHls = () => {
      const hls = hlsRef.current;
      if (!hls) return;
      const rate = Math.max(1, video.playbackRate || 1);
      hls.config.maxBufferLength = HLS_BUFFER_SECONDS * rate;
      hls.config.maxBufferSize = HLS_BUFFER_BYTES * rate;
    };

    video.addEventListener('waiting', onWaiting);
    video.addEventListener('progress', check);
    video.addEventListener('seeking', onSeeking);
    video.addEventListener('play', onPlay);
    video.addEventListener('ratechange', tuneHls);
    video.addEventListener('loadedmetadata', tuneHls);
    return () => {
      video.removeEventListener('waiting', onWaiting);
      video.removeEventListener('progress', check);
      video.removeEventListener('seeking', onSeeking);
      video.removeEventListener('play', onPlay);
      video.removeEventListener('ratechange', tuneHls);
      video.removeEventListener('loadedmetadata', tuneHls);
      release(false);
    };
  }, [enabled, streamIndex, videoRef]);

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
    if (holdingRef.current) {
      holdingRef.current = false;
      setRebuffering(false);
    }
    video.pause();
  }, [videoRef]);

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if ((video.paused || video.ended) && !holdingRef.current) play();
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
    rebuffering,
    play,
    pause,
    togglePlay,
  };
}
