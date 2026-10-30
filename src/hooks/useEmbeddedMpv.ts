import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { MpvMedia } from '../components/player/mpvMedia';
import type { Anime4kPreset } from '../lib/anime4k';
import { buildProxyUrl } from '../lib/streamProxy';
import type { StreamItem, SubtitleItem } from '../types';
import { preferredAudioTrack } from '../components/player/playerLogic';
import type { MediaTrackOption, PlayerEngine } from './usePlayerEngine';
import { defaultSubtitleIndex, SUBTITLES_OFF } from './useSubtitleTracks';
import type { SubtitleOption } from './useSubtitleTracks';
import { t } from '../i18n';
import { addedTrackId, fileSubtitleTracks, providerIndex, subtitleAtLoad, subtitleCorrection } from '../lib/mpvSubtitles';

/** The hidden hardware decoding self-test's stored result. */
export interface HwdecSelfTest {
  ok: boolean;
  driver: string;
  mpv: string;
  app: string;
  /** Unix seconds. */
  at: number;
}

/** `embedded_mpv_status`: whether the embedded player runs, and what the backend knows. */
export interface EmbeddedMpvStatus {
  requested: boolean;
  available: boolean;
  error: string | null;
  /** "Is the video player showing correctly?" must be asked now. */
  ask_confirm: boolean;
  /** Why the web player is used although embedded mpv was chosen before. */
  reason: string | null;
  /** mpv's `hwdec` value in use. */
  hwdec: string;
  selftest: HwdecSelfTest | null;
  running_engine: 'mpv' | 'web';
}

export const MPV_STATUS_KEY = ['embeddedMpvStatus'] as const;

export function useEmbeddedMpvStatus() {
  const queryClient = useQueryClient();
  // The backend reports changes (self-test done, hwdec switched, answer stored).
  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    listen<MpvEvent>(EVENT, ({ payload }) => {
      if (payload.kind === 'status' || payload.kind === 'hwdec_fallback') {
        queryClient.invalidateQueries({ queryKey: MPV_STATUS_KEY });
      }
    }).then((fn) => (cancelled ? fn() : (stop = fn)));
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [queryClient]);
  return useQuery<EmbeddedMpvStatus>({
    queryKey: MPV_STATUS_KEY,
    queryFn: () => invoke<EmbeddedMpvStatus>('embedded_mpv_status'),
    // The GL layer can still fail after start-up, so ask again on every mount.
    staleTime: 0,
    retry: false,
  });
}

/** The only requests the backend accepts (see `embedded_mpv::Action`). */
export type MpvAction =
  | { action: 'load'; url: string; start: number | null }
  | { action: 'add_subtitle'; url: string; title: string | null; select: boolean }
  | { action: 'stop' }
  | { action: 'set_pause'; paused: boolean }
  | { action: 'seek'; seconds: number; absolute: boolean }
  | { action: 'set_volume'; value: number }
  | { action: 'set_mute'; muted: boolean }
  | { action: 'set_speed'; value: number }
  | { action: 'set_audio_track'; id: number | null }
  | { action: 'set_sub_track'; id: number | null }
  | { action: 'set_secondary_sub_track'; id: number | null }
  | { action: 'set_sub_delay'; seconds: number }
  | { action: 'set_sub_scale'; value: number }
  | { action: 'set_sub_pos'; value: number }
  | { action: 'set_secondary_sub_pos'; value: number }
  | { action: 'set_audio_preset'; preset: 'none' | 'dialogue' | 'night' }
  | { action: 'set_video_preset'; preset: 'none' | 'deband' }
  | { action: 'set_fit'; fit: MpvFit }
  | { action: 'set_shaders'; preset: Anime4kPreset }
  | {
      action: 'set_video_rect';
      x: number;
      y: number;
      width: number;
      height: number;
      viewport_width: number;
      viewport_height: number;
    };

export type MpvFit = 'fit' | 'crop' | 'fill';

export function mpvAction(action: MpvAction): Promise<void> {
  return invoke<void>('embedded_mpv', { action });
}

/** A control that cannot apply (nothing loaded yet, a value mpv rejects) is only logged. */
export function sendMpv(action: MpvAction): void {
  mpvAction(action).catch((e) => console.warn('embedded mpv:', action.action, e));
}

export interface MpvTrack {
  id: number | null;
  type: 'video' | 'audio' | 'sub' | null;
  lang: string | null;
  title: string | null;
  codec: string | null;
  selected: boolean;
  default?: boolean;
  /** The URL a track was added from with `add_subtitle`; null for the file's own tracks. */
  external_filename?: string | null;
}

export type MpvEnd = { reason: 'eof' } | { reason: 'error'; error: string };

const EVENT = 'embedded-mpv';

export type MpvEvent =
  | { kind: 'property'; name: string; value: number | boolean | null }
  | { kind: 'tracks'; tracks: MpvTrack[] }
  /** `paused`: mpv's pause state when the file opened (null if it could not be read). */
  | { kind: 'loaded'; tracks: MpvTrack[]; paused?: boolean | null }
  | { kind: 'seeking' | 'seeked' | 'status' | 'hwdec_fallback' }
  | ({ kind: 'end' } & MpvEnd);

function trackLabel(track: MpvTrack, index: number): string {
  return [track.title, track.lang].filter(Boolean).join(' · ') || t('useEmbeddedMpv.trackNumber', { number: index + 1 });
}

export interface MpvEngineOptions {
  media: MpvMedia | null;
  streams: StreamItem[];
  subtitles: SubtitleItem[];
  proxyBase: string;
  /** Loading waits until this is true (the resume position must be known first). */
  enabled: boolean;
  startAt: number;
  startVerified: boolean;
  autoplay: boolean;
  preferredAudioLanguages: string[];
  /** Subtitle languages wanted, most wanted first. */
  subtitleLanguages: string[];
  /** Dubbed audio: subtitle files are added but not shown. */
  startSubtitlesOff: boolean;
  /** No stream would play in mpv; the screen switches to the web player. */
  onFail: (message: string) => void;
}

export interface MpvEngine extends PlayerEngine {
  subtitleOptions: SubtitleOption[];
  selectedSubtitle: string;
  selectSubtitle: (id: string) => void;
  selectedSecondary: string;
  selectSecondary: (id: string) => void;
}

const SUB_ID = 'mpv:';
const trackId = (id: string) => (id.startsWith(SUB_ID) ? Number(id.slice(SUB_ID.length)) : null);

/** Drives the embedded player for one episode, with the same shape as the web engine. */
export function useMpvEngine({
  media,
  streams,
  subtitles,
  proxyBase,
  enabled,
  startAt,
  startVerified,
  autoplay,
  preferredAudioLanguages,
  subtitleLanguages,
  startSubtitlesOff,
  onFail,
}: MpvEngineOptions): MpvEngine {
  const [listening, setListening] = useState(false);
  const [streamIndex, setStreamIndex] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const [tracks, setTracks] = useState<MpvTrack[]>([]);
  const [selectedSubtitle, setSelectedSubtitle] = useState(SUBTITLES_OFF);
  const [selectedSecondary, setSelectedSecondary] = useState(SUBTITLES_OFF);
  const onFailRef = useRef(onFail);
  onFailRef.current = onFail;
  /** Set when no file is added at load: the track mpv then shows becomes the selection. */
  const adoptSubtitle = useRef(false);
  /** The subtitle choice that outlives a reload (undefined until the first file opened). */
  const wantedSubtitle = useRef<string | undefined>(undefined);
  const secondaryRef = useRef(SUBTITLES_OFF);
  /** An added file to show as the second subtitle once mpv lists it. */
  const pendingSecondary = useRef<string | null>(null);
  /** The resume check waits for the file's duration, which mpv reports after it loaded. */
  const resumeCheck = useRef(false);
  // Read once, when loading first begins (as the web engine does).
  const startRef = useRef({ startAt, startVerified, autoplay, preferredAudioLanguages, subtitleLanguages, startSubtitlesOff });

  // The page is see-through while mpv plays under it; playback stops when leaving.
  useEffect(() => {
    if (!media) return;
    document.documentElement.classList.add('mpv-video');
    return () => {
      document.documentElement.classList.remove('mpv-video');
      sendMpv({ action: 'stop' });
    };
  }, [media]);

  useEffect(() => {
    if (!media) return;
    let cancelled = false;
    let stop: (() => void) | undefined;
    listen<MpvEvent>(EVENT, ({ payload }) => media.handle(payload)).then((fn) => {
      if (cancelled) return fn();
      stop = fn;
      setListening(true);
    });
    const onTracks = () => setTracks(media.tracks);
    media.addEventListener('trackschange', onTracks);
    return () => {
      cancelled = true;
      stop?.();
      media.removeEventListener('trackschange', onTracks);
    };
  }, [media]);

  // Load only once the listener is attached, or the first events are missed.
  useEffect(() => {
    if (!media || !listening || !enabled) return;
    const stream = streams[streamIndex];
    if (!stream) {
      onFailRef.current(t('useEmbeddedMpv.noStream'));
      return;
    }
    const start = startRef.current;
    setLoaded(false);
    const url = buildProxyUrl(proxyBase, stream);
    // The stand-in reopens this stream itself for a replay after the end.
    media.src = url;
    sendMpv({ action: 'set_pause', paused: !start.autoplay });
    mpvAction({ action: 'load', url, start: start.startAt > 0 ? start.startAt : null }).catch(
      (e) => onFailRef.current(String(e))
    );
  }, [media, listening, enabled, proxyBase, streams, streamIndex]);

  // A stream mpv cannot open moves on to the next one; after the last, the web player takes over.
  useEffect(() => {
    if (!media) return;
    const onError = () => {
      if (streamIndex < streams.length - 1) setStreamIndex(streamIndex + 1);
      else onFailRef.current(t('useEmbeddedMpv.couldNotPlay', { reason: media.error?.message ?? t('useEmbeddedMpv.unknownError') }));
    };
    media.addEventListener('error', onError);
    return () => media.removeEventListener('error', onError);
  }, [media, streamIndex, streams.length]);

  // The proxy URL of the extension's subtitle file `index` for the current stream. mpv renders
  // the raw file's ASS styling itself.
  const subtitleUrl = useCallback(
    (index: number) => {
      const sub = subtitles[index];
      const stream = streams[streamIndex];
      return sub
        ? buildProxyUrl(proxyBase, { url: sub.url, referer: stream?.referer, origin: stream?.origin, headers: stream?.headers })
        : null;
    },
    [proxyBase, streams, streamIndex, subtitles]
  );

  const subtitleUrlRef = useRef(subtitleUrl);
  subtitleUrlRef.current = subtitleUrl;

  const selectSubtitle = useCallback(
    (id: string) => {
      adoptSubtitle.current = false;
      wantedSubtitle.current = id;
      setSelectedSubtitle(id);
      const index = providerIndex(id);
      const url = index === null ? null : subtitleUrl(index);
      if (index !== null && url) {
        // Added only now that it is picked; mpv reuses it if it was added before.
        sendMpv({ action: 'add_subtitle', url, title: subtitles[index]?.language ?? null, select: true });
      } else {
        sendMpv({ action: 'set_sub_track', id: trackId(id) });
      }
    },
    [subtitleUrl, subtitles]
  );

  const selectSecondary = useCallback(
    (id: string) => {
      secondaryRef.current = id;
      pendingSecondary.current = null;
      setSelectedSecondary(id);
      const index = providerIndex(id);
      const url = index === null ? null : subtitleUrl(index);
      if (index === null || !url || !media) {
        sendMpv({ action: 'set_secondary_sub_track', id: trackId(id) });
        return;
      }
      const added = addedTrackId(media.tracks, url);
      if (added !== null) return sendMpv({ action: 'set_secondary_sub_track', id: added });
      // Added without showing it; the track id is known once mpv lists it (see below).
      pendingSecondary.current = url;
      sendMpv({ action: 'add_subtitle', url, title: subtitles[index]?.language ?? null, select: false });
    },
    [media, subtitleUrl, subtitles]
  );
  const selectSubtitleRef = useRef(selectSubtitle);
  selectSubtitleRef.current = selectSubtitle;
  const selectSecondaryRef = useRef(selectSecondary);
  selectSecondaryRef.current = selectSecondary;

  // Once a file is open: the shown subtitle file, the preferred audio language, a resume check.
  useEffect(() => {
    if (!media) return;
    const onLoaded = () => {
      setLoaded(true);
      const start = startRef.current;
      const step = subtitleAtLoad(
        wantedSubtitle.current,
        SUBTITLES_OFF,
        subtitles.length,
        defaultSubtitleIndex(subtitles, start.subtitleLanguages),
        start.startSubtitlesOff
      );
      // Only the file that is shown is added: mpv downloads each one before showing it.
      if (step.kind === 'add') selectSubtitleRef.current(`ext:${step.index}`);
      else if (step.kind === 'off') selectSubtitleRef.current(SUBTITLES_OFF);
      else adoptSubtitle.current = true;
      // A new file has none of the added files; the second subtitle is added again.
      if (secondaryRef.current !== SUBTITLES_OFF) selectSecondaryRef.current(secondaryRef.current);

      const audio = media.tracks.filter((t) => t.type === 'audio' && t.id !== null);
      if (audio.length > 1) {
        // A dub stream may mark Japanese as its default; the wanted language wins.
        const index = preferredAudioTrack(audio.map((t) => ({ lang: t.lang ?? '', name: t.title ?? '' })), start.preferredAudioLanguages);
        if (index !== -1 && !audio[index].selected) sendMpv({ action: 'set_audio_track', id: audio[index].id });
      }
      // A replay after the end starts where the user asked, not at the saved position.
      resumeCheck.current = !start.startVerified && !media.replaying;
      onDuration();
    };
    const onDuration = () => {
      if (!resumeCheck.current || !Number.isFinite(media.duration)) return;
      resumeCheck.current = false;
      if (startRef.current.startAt >= media.duration * 0.95) media.currentTime = 0;
    };
    media.addEventListener('loadedmetadata', onLoaded);
    media.addEventListener('durationchange', onDuration);
    return () => {
      media.removeEventListener('loadedmetadata', onLoaded);
      media.removeEventListener('durationchange', onDuration);
    };
  }, [media, subtitles]);

  // Follow the file's own track mpv shows when no file was added, and show a second subtitle
  // file once mpv lists it.
  useEffect(() => {
    const pending = pendingSecondary.current;
    const added = pending === null ? null : addedTrackId(tracks, pending);
    if (added !== null) {
      pendingSecondary.current = null;
      sendMpv({ action: 'set_secondary_sub_track', id: added });
    }
    if (!adoptSubtitle.current) {
      // A file that finished downloading after the user picked something else selects itself.
      const wanted = wantedSubtitle.current;
      const fix = wanted === undefined ? undefined : subtitleCorrection(tracks, wanted, subtitleUrlRef.current);
      if (fix !== undefined) sendMpv({ action: 'set_sub_track', id: fix });
      return;
    }
    const shown = fileSubtitleTracks(tracks).find((t) => t.selected);
    if (shown) {
      adoptSubtitle.current = false;
      wantedSubtitle.current = `${SUB_ID}${shown.id}`;
      setSelectedSubtitle(`${SUB_ID}${shown.id}`);
    }
  }, [tracks]);

  // The extension's files (added when picked), then the video file's own tracks.
  const subtitleOptions = useMemo<SubtitleOption[]>(
    () => [
      ...subtitles.map((sub, index) => ({
        id: `ext:${index}`,
        label: sub.language || t('useEmbeddedMpv.trackNumber', { number: index + 1 }),
        format: sub.format || 'sub',
        isDefault: !!sub.is_default,
      })),
      ...fileSubtitleTracks(tracks).map((t, index) => ({
        id: `${SUB_ID}${t.id}`,
        label: trackLabel(t, index),
        format: t.codec ?? 'sub',
        isDefault: !!t.default,
      })),
    ],
    [subtitles, tracks]
  );
  const audioTracks = useMemo<MediaTrackOption[]>(
    () => tracks.filter((t) => t.type === 'audio' && t.id !== null).map((t, index) => ({ id: t.id as number, label: trackLabel(t, index) })),
    [tracks]
  );
  const audioTrack = tracks.find((t) => t.type === 'audio' && t.selected)?.id ?? -1;

  const selectAudioTrack = useCallback((id: number) => sendMpv({ action: 'set_audio_track', id }), []);

  const play = useCallback(() => void media?.play(), [media]);
  const pause = useCallback(() => media?.pause(), [media]);
  const togglePlay = useCallback(() => {
    if (!media) return;
    // After the end, play() opens the stream again from the start.
    if (media.paused || media.ended) media.play();
    else media.pause();
  }, [media]);
  const noop = useCallback(() => {}, []);

  return {
    phase: loaded ? 'ready' : 'loading',
    error: null,
    stream: streams[streamIndex] ?? null,
    usesHls: false,
    levels: [],
    selectedLevel: -1,
    activeLevel: -1,
    selectLevel: noop,
    audioTracks,
    audioTrack,
    selectAudioTrack,
    textTracks: [],
    textTrack: -1,
    selectTextTrack: noop,
    play,
    pause,
    togglePlay,
    rebuffering: false,
    subtitleOptions,
    selectedSubtitle,
    selectSubtitle,
    selectedSecondary,
    selectSecondary,
  };
}
