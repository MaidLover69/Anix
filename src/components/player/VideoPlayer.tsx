import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Captions, Download, ExternalLink, LoaderCircle } from 'lucide-react';
import { formatTime, sameEpisode } from './format';
import { IconButton } from './IconButton';
import { MiniPlayerChrome } from './MiniPlayerChrome';
import { MpvMedia } from './mpvMedia';
import { AUDIO_MODES, PlayerControls } from './PlayerControls';
import type { EpisodeStep } from './PlayerControls';
import { PlayerSettingsPanel } from './PlayerSettingsPanel';
import { PauseDetails, PlayerClock } from './PlayerTopBar';
import { countdownLabel, HOLD_RATE, mpvSubtitlePositions, objectFitClass, PAUSE_DETAILS_MS, stepSpeed } from './playerLogic';
import { SecondarySubtitle } from './SecondarySubtitle';
import type { FeedbackMessage } from './PlayerFeedback';
import type { PlayerMenuOption } from './PlayerSettingsPanel';
import { PlayerStatus } from './PlayerStatus';
import type { PlayerStatusAction } from './PlayerStatus';
import { PlayerTopBar } from './PlayerTopBar';
import { SubtitleSettingsModal } from '../SubtitleSettingsModal';
import { errorMessage } from '../ui/ErrorState';
import { useAutoHide } from '../../hooks/useAutoHide';
import { useDialogueBoost } from '../../hooks/useDialogueBoost';
import { sendMpv, useMpvEngine } from '../../hooks/useEmbeddedMpv';
import { useLanguagePrefs } from '../../hooks/useLanguagePrefs';
import { exitFullscreen } from '../../hooks/useFullscreen';
import { useResumePosition } from '../../hooks/useLibrary';
import { useMediaState } from '../../hooks/useMediaState';
import { supportsPictureInPicture, useMiniPlayer } from '../../hooks/useMiniPlayer';
import { useMpris } from '../../hooks/useMpris';
import { usePlayerEngine } from '../../hooks/usePlayerEngine';
import type { PlayerEngine, QualityLevel } from '../../hooks/usePlayerEngine';
import { usePlayerShortcuts } from '../../hooks/usePlayerShortcuts';
import { useSkipTimes } from '../../hooks/useProviders';
import { useSettings } from '../../hooks/useSettings';
import { useStoredChoice } from '../../hooks/useStoredChoice';
import { useSubtitleDrop } from '../../hooks/useSubtitleDrop';
import { hasSkipTimes, useSkipSegments } from '../../hooks/useSkipSegments';
import type { AutoSkip } from '../../hooks/useSkipSegments';
import { SUBTITLES_OFF, useSubtitleTracks } from '../../hooks/useSubtitleTracks';
import { useWatchProgress } from '../../hooks/useWatchProgress';
import { usePlayerStore } from '../../stores/playerStore';
import { t } from '../../i18n';
import type { SourcesResult } from '../../types';

export interface VideoPlayerProps {
  anilistId: number;
  malId: number | null;
  animeTitle: string;
  animeCover: string | null;
  episodeNumber: number;
  episodeTitle: string | null;
  category: 'sub' | 'dub';
  /** The muted line under the title. */
  metaLine: string;

  sources: SourcesResult;
  /** Key of the provider that served `sources`; recorded with history and downloads. */
  provider: string;
  /** Its display name, when the provider is known. */
  providerName: string | null;
  proxyBase: string;
  /** Identifies this load of the sources; a retry gets a new one. */
  loadKey: string | number;

  autoplay: boolean;
  autoSkip: AutoSkip;
  externalPlayer: 'mpv' | 'vlc';
  /** Embedded mpv draws the video under the page; 'web' plays it in a `<video>`. */
  engineKind: 'web' | 'mpv';
  /** No stream would play in embedded mpv: the screen switches to the web player. */
  onEngineFail: (message: string) => void;
  /** The "External player" engine: hand the episode to mpv/VLC as soon as its stream is known. */
  openExternally: boolean;

  previous: EpisodeStep | null;
  next: EpisodeStep | null;
  queuedCount: number;
  onEnded: () => void;

  onBack: () => void;
  onOpenEpisodes: () => void;
  onOpenServers: () => void;
  /** Resolves the sources again (new stream URLs). */
  onReload: () => void;
  /** Present when a specific provider was requested and others could be tried. */
  onTryOtherServers?: () => void;
  /** Shown after the usual actions when playback fails (a local file: open it elsewhere). */
  extraErrorActions?: PlayerStatusAction[];
  onToggleCategory: (category: 'sub' | 'dub') => void;

  isFullscreen: boolean;
  onToggleFullscreen: () => void;
  /** True while a dialog or drawer of the screen is open. */
  overlayOpen: boolean;
  /** The element shown fullscreen; dialogs are rendered into it. */
  container: HTMLElement | null;
  notify: (message: FeedbackMessage) => void;
}

interface ExternalPlayerProgress {
  anilistId: number;
  episodeNumber: number;
  positionMs: number;
  durationMs: number;
  paused: boolean;
  ended: boolean;
  running: boolean;
}

/** Saved positions this close to the end, or to the start, are not resumed. */
const RESUME_END_FRACTION = 0.95;
const RESUME_MIN_SECONDS = 5;

function resumeStart(saved: [number, number] | null | undefined): { time: number; verified: boolean } {
  if (!saved) return { time: 0, verified: true };
  const position = saved[0] / 1000;
  const duration = saved[1] / 1000;
  if (!(position >= RESUME_MIN_SECONDS)) return { time: 0, verified: true };
  // Rows written by older versions can carry a bogus duration; the engine then checks the
  // position against the real one once the media has loaded.
  if (!(duration > 60)) return { time: position, verified: false };
  return position < duration * RESUME_END_FRACTION
    ? { time: position, verified: true }
    : { time: 0, verified: true };
}

function qualityMenu(levels: QualityLevel[], activeLevel: number): PlayerMenuOption[] {
  if (levels.length < 2) return [];
  const active = levels.find((level) => level.id === activeLevel);
  const heights = levels.map((level) => level.height);
  const label = (level: QualityLevel) => {
    const mbps = t('videoPlayer.mbps', { value: (level.bitrate / 1_000_000).toFixed(1) });
    if (!level.height) return level.bitrate ? mbps : t('videoPlayer.qualityLevel', { number: level.id + 1 });
    const ambiguous = heights.filter((h) => h === level.height).length > 1;
    return ambiguous && level.bitrate ? `${level.height}p · ${mbps}` : `${level.height}p`;
  };
  return [
    { value: '-1', label: active?.height ? t('videoPlayer.autoQuality', { height: active.height }) : t('common.auto') },
    ...[...levels]
      .sort((a, b) => b.height - a.height || b.bitrate - a.bitrate)
      .map((level) => ({ value: String(level.id), label: label(level) })),
  ];
}

export const VideoPlayer: React.FC<VideoPlayerProps> = ({
  anilistId,
  malId,
  animeTitle,
  animeCover,
  episodeNumber,
  episodeTitle,
  category,
  metaLine,
  sources,
  provider,
  providerName,
  proxyBase,
  loadKey,
  autoplay,
  autoSkip,
  externalPlayer,
  engineKind,
  onEngineFail,
  openExternally,
  previous,
  next,
  queuedCount,
  onEnded,
  onBack,
  onOpenEpisodes,
  onOpenServers,
  onReload,
  onTryOtherServers,
  extraErrorActions,
  onToggleCategory,
  isFullscreen,
  onToggleFullscreen,
  overlayOpen,
  container,
  notify,
}) => {
  const videoRef = useRef<HTMLVideoElement>(null);
  const queryClient = useQueryClient();
  // With embedded mpv, `videoRef` holds a stand-in with the element's members and events, so
  // progress, skip times, MPRIS and the controls work unchanged. Hooks that need a real
  // `<video>` (hls.js, `<track>`s, Web Audio) get `elementRef`, which then stays empty.
  const isMpv = engineKind === 'mpv';
  const [mpvMedia] = useState(() => (isMpv ? new MpvMedia(sendMpv) : null));
  if (mpvMedia) (videoRef as React.MutableRefObject<HTMLVideoElement | null>).current = mpvMedia as unknown as HTMLVideoElement;
  const noElementRef = useRef<HTMLVideoElement>(null);
  const elementRef = isMpv ? noElementRef : videoRef;
  const regionRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const languagePrefs = useLanguagePrefs();

  // The saved position has to be known before the stream is loaded.
  const resume = useResumePosition(anilistId, episodeNumber, loadKey);
  const start = useMemo(() => resumeStart(resume.data), [resume.data]);

  const { data: settings } = useSettings();
  // A dub starts in the first preferred dub language the stream has, a sub in Japanese.
  const preferredAudioLanguages = category === 'dub' ? settings?.preferred_dub_languages ?? [] : ['ja'];
  const webEngine = usePlayerEngine({
    videoRef: elementRef,
    streams: sources.streams,
    proxyBase,
    enabled: !isMpv && !resume.isPending,
    startAt: start.time,
    startVerified: start.verified,
    autoplay,
    preferredAudioLanguages,
  });
  const mpvEngine = useMpvEngine({
    media: mpvMedia,
    streams: sources.streams,
    subtitles: sources.subtitles,
    proxyBase,
    enabled: isMpv && !resume.isPending,
    startAt: start.time,
    startVerified: start.verified,
    autoplay,
    preferredAudioLanguages,
    subtitleLanguages: languagePrefs.sub,
    startSubtitlesOff: category === 'dub',
    onFail: onEngineFail,
  });
  const engine: PlayerEngine = isMpv ? mpvEngine : webEngine;
  const media = useMediaState(videoRef);

  const subtitleFontSize = usePlayerStore((s) => s.subtitleFontSize);
  const subtitleRaise = usePlayerStore((s) => s.subtitleRaise);
  const subtitleColor = usePlayerStore((s) => s.subtitleColor);
  const subtitleBackground = usePlayerStore((s) => s.subtitleBackground);
  const subtitleDelayMs = usePlayerStore((s) => s.subtitleDelayMs);
  const secondaryFontSize = usePlayerStore((s) => s.secondarySubtitleFontSize);
  const screenFit = usePlayerStore((s) => s.screenFit);
  const anime4k = usePlayerStore((s) => s.anime4k);

  // A dropped subtitle file becomes a track and is switched on straight away.
  const selectSubtitleRef = useRef<(id: string) => void>(() => {});
  const dropped = useSubtitleDrop(
    (file, index) => {
      if (isMpv) {
        notify({ text: t('videoPlayer.droppedSubtitlesWebOnly'), tone: 'error' });
        return;
      }
      selectSubtitleRef.current(`file:${index}`);
      notify({ text: t('videoPlayer.subtitlesNamed', { name: file.label }) });
    },
    (message) => notify({ text: message, tone: 'error' })
  );

  const webSubtitles = useSubtitleTracks({
    videoRef: elementRef,
    subtitles: sources.subtitles,
    stream: engine.stream,
    proxyBase,
    usesHls: engine.usesHls,
    embeddedTracks: engine.textTracks,
    embeddedTrack: engine.textTrack,
    selectEmbeddedTrack: engine.selectTextTrack,
    delayMs: subtitleDelayMs,
    startOff: category === 'dub',
    localFiles: dropped.files,
  });
  const subtitles = isMpv
    ? { options: mpvEngine.subtitleOptions, selectedId: mpvEngine.selectedSubtitle, select: mpvEngine.selectSubtitle }
    : webSubtitles;
  const selectSubtitle = subtitles.select;
  selectSubtitleRef.current = selectSubtitle;

  // The optional second subtitle, from the same files, drawn at the top of the picture.
  const [webSecondaryId, setWebSecondaryId] = useState(SUBTITLES_OFF);
  const secondaryId = isMpv ? mpvEngine.selectedSecondary : webSecondaryId;
  const setSecondaryId = isMpv ? mpvEngine.selectSecondary : setWebSecondaryId;
  const secondaryOptions = useMemo(
    () =>
      isMpv
        ? subtitles.options.filter((o) => o.id !== subtitles.selectedId)
        : subtitles.options.filter((o) => o.id.startsWith('ext:') || o.id.startsWith('file:')),
    [isMpv, subtitles.options, subtitles.selectedId]
  );
  const secondarySrc = isMpv || webSecondaryId === SUBTITLES_OFF ? null : webSubtitles.fileSrc(webSecondaryId);

  const [audioMode, setAudioMode] = useStoredChoice('player-audio-mode', AUDIO_MODES, 'normal');
  useDialogueBoost(elementRef, !isMpv && audioMode === 'boost');

  const miniPlayer = useMiniPlayer();
  const [pictureInPicture] = useState(() => supportsPictureInPicture(document.createElement('video')));

  // Skip times: the provider's own when it sent any, otherwise AniSkip, asked once with the
  // length this episode turned out to have.
  const providerSkip = hasSkipTimes(sources.skip) ? sources.skip : null;
  const [episodeLength, setEpisodeLength] = useState(0);
  useEffect(() => {
    if (episodeLength === 0 && media.duration > 60) setEpisodeLength(Math.round(media.duration));
  }, [episodeLength, media.duration]);
  const aniSkipPossible = !providerSkip && Number.isInteger(episodeNumber) && malId !== null;
  const aniSkip = useSkipTimes(malId, episodeNumber, episodeLength, aniSkipPossible);
  // The fixed jump is only offered once it is clear that no skip times will come.
  const skipLookupDone = !aniSkipPossible || aniSkip.isFetched;
  const skip = useSkipSegments(videoRef, providerSkip ?? aniSkip.data, autoSkip, skipLookupDone, (kind) =>
    notify({ text: kind === 'intro' ? t('videoPlayer.skippedIntro') : t('videoPlayer.skippedEnding') })
  );

  useWatchProgress(videoRef, {
    anilistId,
    title: animeTitle,
    cover: animeCover,
    episodeNumber,
    episodeTitle,
    provider,
    category,
  });

  const [subtitleSettingsOpen, setSubtitleSettingsOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const panelOpen = subtitleSettingsOpen || settingsOpen || overlayOpen;
  const chrome = useAutoHide((media.playing || engine.rebuffering) && engine.phase === 'ready' && !panelOpen);

  const onEndedRef = useRef(onEnded);
  onEndedRef.current = onEnded;
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const handleEnded = () => {
      // A stream that failed or was torn down while play was requested also fires `ended`;
      // only an episode that actually played to its end may advance to the next one.
      const { duration, currentTime } = video;
      if (Number.isFinite(duration) && duration > 0 && currentTime >= duration - 2) {
        onEndedRef.current();
      }
    };
    video.addEventListener('ended', handleEnded);
    return () => video.removeEventListener('ended', handleEnded);
  }, []);

  const seekTo = useCallback((time: number) => {
    const video = videoRef.current;
    if (video && Number.isFinite(time)) video.currentTime = Math.max(0, time);
  }, []);

  const seekBy = useCallback(
    (seconds: number) => {
      const video = videoRef.current;
      if (!video || !Number.isFinite(video.duration) || video.duration <= 0) return;
      video.currentTime = Math.min(video.duration, Math.max(0, video.currentTime + seconds));
      notify({
        text: t(seconds > 0 ? 'videoPlayer.seekForward' : 'videoPlayer.seekBack', { seconds: Math.abs(seconds) }),
        side: seconds > 0 ? 'right' : 'left',
      });
    },
    [notify]
  );

  // Volume and mute are written to the element only; the UI follows its `volumechange`.
  const setVolume = useCallback((volume: number) => {
    const video = videoRef.current;
    if (!video) return;
    const next = Math.min(1, Math.max(0, Math.round(volume * 100) / 100));
    video.volume = next;
    if (next > 0 && video.muted) video.muted = false;
  }, []);

  const volumeBy = useCallback(
    (delta: number) => {
      const video = videoRef.current;
      if (!video) return;
      const next = Math.min(1, Math.max(0, (video.muted ? 0 : video.volume) + delta));
      setVolume(next);
      notify({ text: t('videoPlayer.volumePercent', { percent: Math.round(next * 100) }) });
    },
    [notify, setVolume]
  );

  const toggleMute = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.muted || video.volume === 0) {
      video.muted = false;
      if (video.volume === 0) video.volume = 0.5;
      notify({ text: t('videoPlayer.volumePercent', { percent: Math.round(video.volume * 100) }) });
    } else {
      video.muted = true;
      notify({ text: t('videoPlayer.muted') });
    }
  }, [notify]);

  const setRate = useCallback((rate: number) => {
    const video = videoRef.current;
    if (!video || !(rate > 0)) return;
    usePlayerStore.getState().setPlaybackRate(rate);
    video.defaultPlaybackRate = rate;
    video.playbackRate = rate;
  }, []);

  const speedBy = useCallback(
    (direction: 1 | -1) => {
      const next = stepSpeed(videoRef.current?.playbackRate ?? 1, direction);
      setRate(next);
      notify({ text: t('videoPlayer.speed', { rate: Number(next.toFixed(2)) }) });
    },
    [setRate, notify]
  );

  // Holding Space plays at 2x and lets go back to the speed from before.
  const heldFrom = useRef<number | null>(null);
  const holdStart = useCallback(() => {
    const video = videoRef.current;
    if (!video || heldFrom.current !== null) return;
    heldFrom.current = video.playbackRate || 1;
    if (video.paused) engine.play();
    video.playbackRate = HOLD_RATE;
    notify({ text: t('videoPlayer.holdSpeed', { rate: HOLD_RATE }) });
  }, [engine, notify]);
  const holdEnd = useCallback(() => {
    const video = videoRef.current;
    const previousRate = heldFrom.current;
    heldFrom.current = null;
    if (video && previousRate !== null) video.playbackRate = previousRate;
  }, []);

  const handleSelectSubtitle = useCallback(
    (id: string) => {
      selectSubtitle(id);
      const option = subtitles.options.find((o) => o.id === id);
      notify({
        text:
          id === SUBTITLES_OFF
            ? t('videoPlayer.subtitlesOff')
            : option
              ? t('videoPlayer.subtitlesNamed', { name: option.label })
              : t('videoPlayer.subtitlesOn'),
      });
    },
    [selectSubtitle, subtitles.options, notify]
  );

  const handleSubtitleError = useCallback(() => {
    selectSubtitle(SUBTITLES_OFF);
    notify({ text: t('videoPlayer.subtitleLoadFailed'), tone: 'error' });
  }, [selectSubtitle, notify]);

  const handleDownload = () => {
    const stream = engine.stream;
    if (!stream) return;
    invoke('start_episode_download', {
      // Status, progress and timestamps are set by the backend.
      record: {
        id: `dl-${anilistId}-${episodeNumber}-${Date.now()}`,
        anilist_id: anilistId,
        episode_num: episodeNumber,
        episode_title: episodeTitle ?? `Episode ${episodeNumber}`,
        series_title: animeTitle,
        series_cover: animeCover,
        provider,
        category,
        quality: stream.quality || 'auto',
        status: 'QUEUED',
        progress: 0,
        file_path: null,
        file_size: null,
        duration_s: media.duration > 0 ? media.duration : null,
        error_msg: null,
        created_at: 0,
        updated_at: 0,
      },
      streamUrl: stream.url,
      referer: stream.referer ?? null,
      origin: stream.origin ?? null,
      subtitles: sources.subtitles,
      headers: stream.headers ?? null,
      streamType: stream.stream_type || null,
    })
      .then(() => {
        notify({ text: t('videoPlayer.downloadStarted', { number: episodeNumber }) });
        queryClient.invalidateQueries({ queryKey: ['allDownloads'] });
      })
      .catch((err) =>
        notify({ text: t('videoPlayer.downloadFailed', { error: errorMessage(err) }), tone: 'error' })
      );
  };

  const handleOpenExternal = () => {
    const stream = engine.stream;
    if (!stream) return;
    // The subtitle shown here goes first so the external player starts with it.
    const selected = subtitles.selectedId.startsWith('ext:') ? Number(subtitles.selectedId.slice(4)) : -1;
    const ordered = sources.subtitles
      .map((sub, index) => ({ sub, index }))
      .sort((a, b) => Number(b.index === selected) - Number(a.index === selected))
      .map(({ sub }) => ({ url: sub.url, language: sub.language }));
    // mpv starts where this player is (or at the saved position) and reports its own back.
    const video = videoRef.current;
    const here = video && video.readyState > 0 && video.currentTime > 0 ? video.currentTime : start.time;
    invoke('launch_external_player', {
      player: externalPlayer,
      url: stream.url,
      title: `${animeTitle} - Episode ${episodeNumber}`,
      referer: stream.referer ?? null,
      subtitles: ordered,
      subtitlesOn: subtitles.selectedId !== SUBTITLES_OFF,
      episode: {
        anilistId,
        title: animeTitle,
        cover: animeCover,
        episodeNumber,
        episodeTitle,
        provider,
        category,
      },
      startSeconds: here > 0 ? here : null,
    })
      .then(() => {
        engine.pause();
        notify({ text: t('videoPlayer.openedIn', { player: externalPlayer === 'vlc' ? 'VLC' : 'mpv' }) });
      })
      .catch((err) =>
        notify({ text: t('videoPlayer.externalPlayerFailed', { error: errorMessage(err) }), tone: 'error' })
      );
  };

  // Progress made in mpv is saved by the backend; this screen follows it.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    listen<ExternalPlayerProgress>('external-player-progress', ({ payload }) => {
      if (payload.anilistId !== anilistId || !sameEpisode(payload.episodeNumber, episodeNumber)) return;
      queryClient.invalidateQueries({ queryKey: ['continueWatching'] });
      queryClient.invalidateQueries({ queryKey: ['homeData'] });
      queryClient.invalidateQueries({ queryKey: ['watchedEpisodes', anilistId] });
      if (payload.running) return;
      if (payload.ended) {
        notify({ text: t('videoPlayer.finishedInMpv', { number: episodeNumber }) });
      } else if (payload.positionMs > 0) {
        const seconds = payload.positionMs / 1000;
        const video = videoRef.current;
        if (video && video.readyState > 0) video.currentTime = seconds;
        notify({ text: t('videoPlayer.backFromMpv', { time: formatTime(seconds) }) });
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [anilistId, episodeNumber, queryClient, notify]);

  useMpris(
    videoRef,
    {
      title: t('videoPlayer.titleWithEpisode', { title: animeTitle, number: episodeNumber }),
      album: animeTitle,
      artUrl: animeCover,
      track: `${anilistId}-${episodeNumber}-${category}`,
      canNext: next !== null,
      canPrevious: previous !== null,
    },
    {
      play: engine.play,
      pause: engine.pause,
      togglePlay: engine.togglePlay,
      next: () => next?.onClick(),
      previous: () => previous?.onClick(),
      seekTo: (time) => {
        const video = videoRef.current;
        if (!video) return;
        const end = Number.isFinite(video.duration) && video.duration > 0 ? video.duration : time;
        seekTo(Math.min(end, time));
      },
      setVolume,
      setRate,
    }
  );

  const handlePictureInPicture = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    const request = document.pictureInPictureElement === video
      ? document.exitPictureInPicture()
      : video.requestPictureInPicture();
    Promise.resolve(request).catch(() => notify({ text: t('videoPlayer.pipUnavailable'), tone: 'error' }));
  }, [notify]);

  usePlayerShortcuts({
    speedBy,
    holdStart,
    holdEnd,
    confirmSkip: () => {
      if (skip.countdown === null) return false;
      skip.skipActive();
      return true;
    },
    cancelSkip: skip.cancelCountdown,
    togglePlay: engine.togglePlay,
    seekBy,
    volumeBy,
    toggleMute,
    toggleFullscreen: onToggleFullscreen,
    exitFullscreen,
    nextEpisode: () => next?.onClick(),
    previousEpisode: () => previous?.onClick(),
    skipSegment: skip.active ? skip.skipActive : skip.skipFallback,
    toggleMiniPlayer: miniPlayer.toggle,
  });

  // Embedded mpv: the page's choices applied to mpv.
  useEffect(() => {
    if (isMpv) sendMpv({ action: 'set_sub_delay', seconds: subtitleDelayMs / 1000 });
  }, [isMpv, subtitleDelayMs]);
  useEffect(() => {
    // The web player's default text size is 20 px.
    if (isMpv) sendMpv({ action: 'set_sub_scale', value: subtitleFontSize / 20 });
  }, [isMpv, subtitleFontSize]);
  useEffect(() => {
    if (isMpv) sendMpv({ action: 'set_audio_preset', preset: audioMode === 'boost' ? 'dialogue' : 'none' });
  }, [isMpv, audioMode]);
  useEffect(() => {
    if (isMpv) sendMpv({ action: 'set_fit', fit: screenFit });
  }, [isMpv, screenFit]);
  useEffect(() => {
    if (isMpv) sendMpv({ action: 'set_shaders', preset: anime4k });
  }, [isMpv, anime4k]);

  // Embedded mpv: keep the video inside the player's region, and its subtitles clear of the
  // bars while they show (raised by the "Subtitle height" setting otherwise).
  useEffect(() => {
    const region = regionRef.current;
    const root = rootRef.current;
    if (!isMpv || !region || !root) return;
    let lastPositions = '';
    const report = () => {
      const r = region.getBoundingClientRect();
      if (r.width < 1 || r.height < 1) return;
      sendMpv({
        action: 'set_video_rect',
        x: r.left,
        y: r.top,
        width: r.width,
        height: r.height,
        viewport_width: window.innerWidth,
        viewport_height: window.innerHeight,
      });
      const bar = (name: string) =>
        root.querySelector<HTMLElement>(`[data-player-bar="${name}"]`)?.getBoundingClientRect().height ?? 0;
      const positions = mpvSubtitlePositions({
        regionHeight: r.height,
        bottomBar: bar('bottom'),
        topBar: bar('top'),
        raisePercent: subtitleRaise,
        chromeVisible: chrome.visible,
      });
      const key = `${positions.primary}:${positions.secondary}`;
      if (key === lastPositions) return;
      lastPositions = key;
      sendMpv({ action: 'set_sub_pos', value: positions.primary });
      sendMpv({ action: 'set_secondary_sub_pos', value: positions.secondary });
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(region);
    window.addEventListener('resize', report);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', report);
    };
  }, [isMpv, subtitleRaise, chrome.visible, miniPlayer.mini]);

  const qualityOptions = useMemo(
    () => qualityMenu(engine.levels, engine.activeLevel),
    [engine.levels, engine.activeLevel]
  );

  const errorActions: PlayerStatusAction[] = [
    { label: t('common.retry'), onClick: onReload, primary: true },
    { label: t('videoPlayer.changeServer'), onClick: onOpenServers },
    ...(onTryOtherServers ? [{ label: t('videoPlayer.tryOtherServers'), onClick: onTryOtherServers }] : []),
    ...(extraErrorActions ?? []),
  ];

  const failed = engine.phase === 'error';
  const loading = engine.phase === 'loading';
  const hasStream = engine.stream !== null && !failed;

  // The "External player" engine hands each episode over as soon as its stream is known.
  const openExternalRef = useRef(handleOpenExternal);
  openExternalRef.current = handleOpenExternal;
  const openedExternally = useRef(false);
  useEffect(() => {
    if (!openExternally || !hasStream || openedExternally.current) return;
    openedExternally.current = true;
    openExternalRef.current();
  }, [openExternally, hasStream]);

  // A long pause shows what is playing over the picture; playing hides it again.
  const pausedNow = !media.playing && !engine.rebuffering && engine.phase === 'ready' && !failed;
  const [pausedLong, setPausedLong] = useState(false);
  useEffect(() => {
    setPausedLong(false);
    if (!pausedNow) return;
    const timer = window.setTimeout(() => setPausedLong(true), PAUSE_DETAILS_MS);
    return () => window.clearTimeout(timer);
  }, [pausedNow]);

  return (
    <div
      ref={rootRef}
      // Time, seek bar and the left/right seek zones read left to right in every language.
      dir="ltr"
      className={`absolute inset-0 select-none ${chrome.visible ? '' : 'cursor-none'}`}
      onMouseMove={chrome.poke}
      onKeyDownCapture={chrome.poke}
      onFocusCapture={chrome.poke}
    >
      {isMpv ? (
        // mpv draws under the page here; this region stays see-through.
        <div ref={regionRef} aria-hidden="true" className="absolute inset-0" onClick={engine.togglePlay} />
      ) : (
        <video
          ref={videoRef}
          crossOrigin="anonymous"
          playsInline
          onClick={engine.togglePlay}
          className={`anilili-video h-full w-full bg-black ${objectFitClass(screenFit)} ${
            chrome.visible ? 'anilili-chrome-visible' : ''
          }`}
        >
          {webSubtitles.trackSrc && (
            <SubtitleTrack key={webSubtitles.trackSrc} src={webSubtitles.trackSrc} onError={handleSubtitleError} />
          )}
        </video>
      )}

      {secondarySrc && !isMpv && (
        <SecondarySubtitle
          key={secondarySrc}
          videoRef={videoRef}
          src={secondarySrc}
          fontSize={secondaryFontSize}
          color={subtitleColor}
          background={subtitleBackground}
          delayMs={subtitleDelayMs}
          onError={() => {
            setWebSecondaryId(SUBTITLES_OFF);
            notify({ text: t('videoPlayer.subtitleLoadFailed'), tone: 'error' });
          }}
        />
      )}

      {dropped.dragging && (
        <div className="pointer-events-none absolute inset-4 z-30 flex flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-white/60 bg-black/60 text-white">
          <Captions aria-hidden="true" className="h-8 w-8" />
          <span className="text-sm font-medium">{t('videoPlayer.dropSubtitle')}</span>
        </div>
      )}

      <style>{`
        .anilili-video::cue {
          font-size: ${subtitleFontSize}px !important;
          color: ${subtitleColor} !important;
          background-color: ${subtitleBackground} !important;
          font-family: inherit !important;
          text-shadow: 0 0 4px #000, 0 0 8px #000 !important;
        }
        /* Subtitles are laid out against the bottom of the cue area, so shortening it raises
           them: by the chosen height, and at least above the control bar while it shows. */
        .anilili-video::-webkit-media-text-track-container {
          height: calc(100% - ${subtitleRaise}%) !important;
        }
        .anilili-video.anilili-chrome-visible::-webkit-media-text-track-container {
          height: calc(100% - max(${subtitleRaise}%, 80px)) !important;
        }
      `}</style>

      {failed ? (
        <PlayerStatus
          scrim
          kind="error"
          title={providerName ? t('videoPlayer.playbackFailedOn', { provider: providerName }) : t('videoPlayer.playbackFailed')}
          detail={engine.error}
          actions={errorActions}
        />
      ) : loading ? (
        <PlayerStatus kind="loading" title={t('videoPlayer.loadingStream')} />
      ) : (
        (media.buffering || engine.rebuffering) && (
          <div role="status" className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center">
            <LoaderCircle aria-hidden="true" className="h-10 w-10 animate-spin text-white" />
            <span className="sr-only">{t('videoPlayer.buffering')}</span>
          </div>
        )
      )}

      {skip.active && !failed && !miniPlayer.mini && (
        <div className="absolute bottom-28 right-6 z-20 flex items-center gap-1">
          <button
            type="button"
            onClick={skip.skipActive}
            onMouseDown={(e) => e.preventDefault()}
            title={
              skip.countdown !== null
                ? t('videoPlayer.skipNowWithKey')
                : skip.active.kind === 'intro'
                  ? t('videoPlayer.skipIntroWithKey')
                  : t('videoPlayer.skipEndingWithKey')
            }
            className="h-9 rounded-md border border-white/20 bg-black/70 px-4 text-[13px] font-medium tabular-nums text-white cursor-pointer transition-colors hover:bg-black/90"
          >
            {skip.countdown !== null
              ? t(skip.active.kind === 'intro' ? 'videoPlayer.skipIntroIn' : 'videoPlayer.skipEndingIn', {
                  seconds: countdownLabel(skip.countdown),
                })
              : t(skip.active.kind === 'intro' ? 'videoPlayer.skipIntro' : 'videoPlayer.skipEnding')}
          </button>
          {skip.countdown !== null && (
            <button
              type="button"
              onClick={skip.cancelCountdown}
              onMouseDown={(e) => e.preventDefault()}
              title={t('videoPlayer.keepWatchingWithKey')}
              className="h-9 rounded-md border border-white/20 bg-black/70 px-3 text-[13px] text-white cursor-pointer transition-colors hover:bg-black/90"
            >
              {t('common.cancel')}
            </button>
          )}
        </div>
      )}
      <span className="sr-only" aria-live="polite">
        {skip.countdown !== null && skip.active
          ? t(skip.active.kind === 'intro' ? 'videoPlayer.skippingIntroAnnounce' : 'videoPlayer.skippingEndingAnnounce')
          : ''}
      </span>
      {!skip.active && skip.fallbackShown && !failed && !miniPlayer.mini && (
        <button
          type="button"
          onClick={skip.skipFallback}
          onMouseDown={(e) => e.preventDefault()}
          title={t('videoPlayer.fallbackSkipHint')}
          className="absolute bottom-28 right-6 z-20 h-9 rounded-md border border-white/20 bg-black/70 px-4 text-[13px] font-medium text-white cursor-pointer transition-colors hover:bg-black/90"
        >
          {t('videoPlayer.fallbackSkip')}
        </button>
      )}

      {pausedLong && !panelOpen && !miniPlayer.mini && (
        <PauseDetails title={animeTitle} episode={episodeNumber} episodeTitle={episodeTitle} meta={metaLine} />
      )}

      {miniPlayer.mini ? (
        <MiniPlayerChrome
          videoRef={videoRef}
          visible={chrome.visible}
          title={t('videoPlayer.titleWithEpisode', { title: animeTitle, number: episodeNumber })}
          playing={media.playing || engine.rebuffering}
          duration={media.duration}
          onTogglePlay={engine.togglePlay}
          onSeek={seekTo}
          next={next}
          onExit={miniPlayer.exit}
        />
      ) : (
        <>
          <PlayerTopBar title={animeTitle} meta={metaLine} onBack={onBack} visible={chrome.visible}>
            <PlayerClock videoRef={videoRef} />
            <IconButton
              label={t('videoPlayer.download')}
              title={
                provider === 'local'
                  ? t('videoPlayer.downloadLocal')
                  : t('videoPlayer.downloadHint')
              }
              disabled={!hasStream || provider === 'local'}
              onClick={handleDownload}
            >
              <Download aria-hidden="true" className="h-4 w-4" />
            </IconButton>
            <IconButton
              label={t('videoPlayer.openIn', { player: externalPlayer === 'vlc' ? 'VLC' : 'mpv' })}
              disabled={!hasStream}
              onClick={handleOpenExternal}
            >
              <ExternalLink aria-hidden="true" className="h-4 w-4" />
            </IconButton>
          </PlayerTopBar>

          <PlayerControls
            videoRef={videoRef}
            visible={chrome.visible}
            playing={media.playing || engine.rebuffering}
            duration={media.duration}
            onTogglePlay={engine.togglePlay}
            onSeek={seekTo}
            onSeekBy={seekBy}
            previous={previous}
            next={next}
            volume={media.volume}
            muted={media.muted}
            onVolumeChange={setVolume}
            onToggleMute={toggleMute}
            subtitlesOn={subtitles.selectedId !== SUBTITLES_OFF}
            onOpenSubtitles={() => setSubtitleSettingsOpen(true)}
            onOpenEpisodes={onOpenEpisodes}
            queuedCount={queuedCount}
            onOpenServers={onOpenServers}
            onOpenSettings={() => setSettingsOpen(true)}
            isFullscreen={isFullscreen}
            onToggleFullscreen={onToggleFullscreen}
            onToggleMiniPlayer={miniPlayer.toggle}
            onPictureInPicture={pictureInPicture && !isMpv ? handlePictureInPicture : undefined}
          />
        </>
      )}

      <SubtitleSettingsModal
        isOpen={subtitleSettingsOpen}
        onClose={() => setSubtitleSettingsOpen(false)}
        subtitles={subtitles.options}
        selectedSubtitleId={subtitles.selectedId}
        onSelectSubtitle={handleSelectSubtitle}
        secondarySubtitles={secondaryOptions}
        selectedSecondaryId={secondaryId}
        onSelectSecondary={setSecondaryId}
        audioTracks={engine.audioTracks}
        selectedAudioTrackId={engine.audioTrack}
        onSelectAudioTrack={engine.selectAudioTrack}
        category={category}
        onToggleCategory={onToggleCategory}
        container={container}
        engine={engineKind}
      />

      <PlayerSettingsPanel
        isOpen={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        container={container}
        engine={engineKind}
        rate={media.rate}
        onRateChange={setRate}
        qualityOptions={qualityOptions}
        quality={String(engine.selectedLevel)}
        onQualityChange={(value) => engine.selectLevel(Number(value))}
        audioMode={audioMode}
        onAudioModeChange={(mode) => {
          setAudioMode(mode);
          notify({ text: mode === 'boost' ? t('videoPlayer.dialogueBoostOn') : t('videoPlayer.dialogueBoostOff') });
        }}
      />
    </div>
  );
};

interface SubtitleTrackProps {
  src: string;
  onError: () => void;
}

/**
 * The one `<track>` of the selected subtitle file. It has no label on purpose: hls.js
 * manages every labelled text track on the element and would switch this one off.
 */
const SubtitleTrack: React.FC<SubtitleTrackProps> = ({ src, onError }) => {
  const ref = useRef<HTMLTrackElement>(null);
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const handleError = () => onErrorRef.current();
    element.addEventListener('error', handleError);
    // A track added after the media loads stays disabled until it is switched on.
    element.track.mode = 'showing';
    return () => element.removeEventListener('error', handleError);
  }, []);

  return <track ref={ref} kind="subtitles" src={src} default />;
};
