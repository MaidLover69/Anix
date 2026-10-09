import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Download, ExternalLink, LoaderCircle } from 'lucide-react';
import { IconButton } from './IconButton';
import { PlayerControls } from './PlayerControls';
import type { EpisodeStep } from './PlayerControls';
import type { FeedbackMessage } from './PlayerFeedback';
import type { PlayerMenuOption } from './PlayerMenu';
import { PlayerStatus } from './PlayerStatus';
import type { PlayerStatusAction } from './PlayerStatus';
import { PlayerTopBar } from './PlayerTopBar';
import { SubtitleSettingsModal } from '../SubtitleSettingsModal';
import { errorMessage } from '../ui/ErrorState';
import { useAutoHide } from '../../hooks/useAutoHide';
import { exitFullscreen } from '../../hooks/useFullscreen';
import { useResumePosition } from '../../hooks/useLibrary';
import { useMediaState } from '../../hooks/useMediaState';
import { usePlayerEngine } from '../../hooks/usePlayerEngine';
import type { QualityLevel } from '../../hooks/usePlayerEngine';
import { usePlayerShortcuts } from '../../hooks/usePlayerShortcuts';
import { useSkipTimes } from '../../hooks/useProviders';
import { hasSkipTimes, useSkipSegments } from '../../hooks/useSkipSegments';
import { SUBTITLES_OFF, useSubtitleTracks } from '../../hooks/useSubtitleTracks';
import { useWatchProgress } from '../../hooks/useWatchProgress';
import { usePlayerStore } from '../../stores/playerStore';
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
  autoSkip: boolean;
  externalPlayer: 'mpv' | 'vlc';

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
  onToggleCategory: (category: 'sub' | 'dub') => void;

  isFullscreen: boolean;
  onToggleFullscreen: () => void;
  /** True while a dialog or drawer of the screen is open. */
  overlayOpen: boolean;
  /** The element shown fullscreen; dialogs are rendered into it. */
  container: HTMLElement | null;
  notify: (message: FeedbackMessage) => void;
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
    const mbps = `${(level.bitrate / 1_000_000).toFixed(1)} Mbps`;
    if (!level.height) return level.bitrate ? mbps : `Level ${level.id + 1}`;
    const ambiguous = heights.filter((h) => h === level.height).length > 1;
    return ambiguous && level.bitrate ? `${level.height}p · ${mbps}` : `${level.height}p`;
  };
  return [
    { value: '-1', label: active?.height ? `Auto (${active.height}p)` : 'Auto' },
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
  previous,
  next,
  queuedCount,
  onEnded,
  onBack,
  onOpenEpisodes,
  onOpenServers,
  onReload,
  onTryOtherServers,
  onToggleCategory,
  isFullscreen,
  onToggleFullscreen,
  overlayOpen,
  container,
  notify,
}) => {
  const videoRef = useRef<HTMLVideoElement>(null);
  const queryClient = useQueryClient();

  // The saved position has to be known before the stream is loaded.
  const resume = useResumePosition(anilistId, episodeNumber, loadKey);
  const start = useMemo(() => resumeStart(resume.data), [resume.data]);

  const engine = usePlayerEngine({
    videoRef,
    streams: sources.streams,
    proxyBase,
    enabled: !resume.isPending,
    startAt: start.time,
    startVerified: start.verified,
    autoplay,
  });
  const media = useMediaState(videoRef);

  const subtitleFontSize = usePlayerStore((s) => s.subtitleFontSize);
  const subtitleColor = usePlayerStore((s) => s.subtitleColor);
  const subtitleBackground = usePlayerStore((s) => s.subtitleBackground);
  const subtitleDelayMs = usePlayerStore((s) => s.subtitleDelayMs);

  const subtitles = useSubtitleTracks({
    videoRef,
    subtitles: sources.subtitles,
    stream: engine.stream,
    proxyBase,
    usesHls: engine.usesHls,
    embeddedTracks: engine.textTracks,
    embeddedTrack: engine.textTrack,
    selectEmbeddedTrack: engine.selectTextTrack,
    delayMs: subtitleDelayMs,
    startOff: category === 'dub',
  });
  const selectSubtitle = subtitles.select;

  // Skip times: the provider's own when it sent any, otherwise AniSkip, asked once with the
  // length this episode turned out to have.
  const providerSkip = hasSkipTimes(sources.skip) ? sources.skip : null;
  const [episodeLength, setEpisodeLength] = useState(0);
  useEffect(() => {
    if (episodeLength === 0 && media.duration > 60) setEpisodeLength(Math.round(media.duration));
  }, [episodeLength, media.duration]);
  const aniSkip = useSkipTimes(
    malId,
    episodeNumber,
    episodeLength,
    !providerSkip && Number.isInteger(episodeNumber)
  );
  const skip = useSkipSegments(videoRef, providerSkip ?? aniSkip.data, autoSkip, (kind) =>
    notify({ text: kind === 'intro' ? 'Skipped the intro' : 'Skipped the ending' })
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
  const [menuOpen, setMenuOpen] = useState(false);
  const chrome = useAutoHide(
    media.playing && engine.phase === 'ready' && !menuOpen && !subtitleSettingsOpen && !overlayOpen
  );

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
        text: `${seconds > 0 ? '+' : '−'}${Math.abs(seconds)}s`,
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
      notify({ text: `Volume ${Math.round(next * 100)}%` });
    },
    [notify, setVolume]
  );

  const toggleMute = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.muted || video.volume === 0) {
      video.muted = false;
      if (video.volume === 0) video.volume = 0.5;
      notify({ text: `Volume ${Math.round(video.volume * 100)}%` });
    } else {
      video.muted = true;
      notify({ text: 'Muted' });
    }
  }, [notify]);

  const setRate = useCallback((rate: number) => {
    const video = videoRef.current;
    if (!video || !(rate > 0)) return;
    usePlayerStore.getState().setPlaybackRate(rate);
    video.defaultPlaybackRate = rate;
    video.playbackRate = rate;
  }, []);

  const handleSelectSubtitle = useCallback(
    (id: string) => {
      selectSubtitle(id);
      const option = subtitles.options.find((o) => o.id === id);
      notify({ text: id === SUBTITLES_OFF ? 'Subtitles off' : `Subtitles: ${option?.label ?? 'on'}` });
    },
    [selectSubtitle, subtitles.options, notify]
  );

  const handleSubtitleError = useCallback(() => {
    selectSubtitle(SUBTITLES_OFF);
    notify({ text: 'That subtitle track could not be loaded', tone: 'error' });
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
        notify({ text: `Download started: Episode ${episodeNumber}. See Library › Downloads.` });
        queryClient.invalidateQueries({ queryKey: ['allDownloads'] });
      })
      .catch((err) =>
        notify({ text: `Download failed: ${errorMessage(err)}`, tone: 'error' })
      );
  };

  const handleOpenExternal = () => {
    const stream = engine.stream;
    if (!stream) return;
    invoke('launch_external_player', {
      player: externalPlayer,
      url: stream.url,
      title: `${animeTitle} - Episode ${episodeNumber}`,
      referer: stream.referer ?? null,
    })
      .then(() => {
        engine.pause();
        notify({ text: `Opened in ${externalPlayer === 'vlc' ? 'VLC' : 'mpv'}` });
      })
      .catch((err) =>
        notify({ text: `Could not open the external player: ${errorMessage(err)}`, tone: 'error' })
      );
  };

  usePlayerShortcuts({
    togglePlay: engine.togglePlay,
    seekBy,
    volumeBy,
    toggleMute,
    toggleFullscreen: onToggleFullscreen,
    exitFullscreen,
    nextEpisode: () => next?.onClick(),
    previousEpisode: () => previous?.onClick(),
    skipSegment: skip.skipActive,
  });

  const qualityOptions = useMemo(
    () => qualityMenu(engine.levels, engine.activeLevel),
    [engine.levels, engine.activeLevel]
  );

  const errorActions: PlayerStatusAction[] = [
    { label: 'Retry', onClick: onReload, primary: true },
    { label: 'Change server', onClick: onOpenServers },
    ...(onTryOtherServers ? [{ label: 'Try other servers', onClick: onTryOtherServers }] : []),
  ];

  const failed = engine.phase === 'error';
  const loading = engine.phase === 'loading';
  const hasStream = engine.stream !== null && !failed;

  return (
    <div
      className={`absolute inset-0 select-none ${chrome.visible ? '' : 'cursor-none'}`}
      onMouseMove={chrome.poke}
      onFocusCapture={chrome.poke}
    >
      <video
        ref={videoRef}
        crossOrigin="anonymous"
        playsInline
        onClick={engine.togglePlay}
        className={`anilili-video h-full w-full bg-black object-contain ${
          chrome.visible ? 'anilili-chrome-visible' : ''
        }`}
      >
        {subtitles.trackSrc && (
          <SubtitleTrack key={subtitles.trackSrc} src={subtitles.trackSrc} onError={handleSubtitleError} />
        )}
      </video>

      <style>{`
        .anilili-video::cue {
          font-size: ${subtitleFontSize}px !important;
          color: ${subtitleColor} !important;
          background-color: ${subtitleBackground} !important;
          font-family: inherit !important;
          text-shadow: 0 0 4px #000, 0 0 8px #000 !important;
        }
        /* Subtitles are laid out against the bottom of the element, which is where the control
           bar sits. While the bar is showing, the cue area ends above it. */
        .anilili-video.anilili-chrome-visible::-webkit-media-text-track-container {
          height: calc(100% - 80px) !important;
        }
      `}</style>

      {failed ? (
        <PlayerStatus
          scrim
          kind="error"
          title={providerName ? `Playback failed on ${providerName}` : 'Playback failed'}
          detail={engine.error}
          actions={errorActions}
        />
      ) : loading ? (
        <PlayerStatus kind="loading" title="Loading the stream…" />
      ) : (
        media.buffering && (
          <div role="status" className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center">
            <LoaderCircle aria-hidden="true" className="h-10 w-10 animate-spin text-white" />
            <span className="sr-only">Buffering</span>
          </div>
        )
      )}

      {skip.active && !failed && (
        <button
          type="button"
          onClick={skip.skipActive}
          onMouseDown={(e) => e.preventDefault()}
          title={skip.active.kind === 'intro' ? 'Skip intro (S)' : 'Skip ending (S)'}
          className="absolute bottom-28 right-6 z-20 h-9 rounded-md border border-white/20 bg-black/70 px-4 text-[13px] font-medium text-white cursor-pointer transition-colors hover:bg-black/90"
        >
          {skip.active.kind === 'intro' ? 'Skip intro' : 'Skip ending'}
        </button>
      )}

      <PlayerTopBar title={animeTitle} meta={metaLine} onBack={onBack} visible={chrome.visible}>
        <IconButton
          label="Download this episode"
          title="Download this episode (MKV with subtitles)"
          disabled={!hasStream}
          onClick={handleDownload}
        >
          <Download aria-hidden="true" className="h-4 w-4" />
        </IconButton>
        <IconButton
          label={`Open in ${externalPlayer === 'vlc' ? 'VLC' : 'mpv'}`}
          disabled={!hasStream}
          onClick={handleOpenExternal}
        >
          <ExternalLink aria-hidden="true" className="h-4 w-4" />
        </IconButton>
      </PlayerTopBar>

      <PlayerControls
        videoRef={videoRef}
        visible={chrome.visible}
        playing={media.playing}
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
        rate={media.rate}
        onRateChange={setRate}
        qualityOptions={qualityOptions}
        quality={String(engine.selectedLevel)}
        onQualityChange={(value) => engine.selectLevel(Number(value))}
        subtitlesOn={subtitles.selectedId !== SUBTITLES_OFF}
        onOpenSubtitles={() => setSubtitleSettingsOpen(true)}
        onOpenEpisodes={onOpenEpisodes}
        queuedCount={queuedCount}
        onOpenServers={onOpenServers}
        isFullscreen={isFullscreen}
        onToggleFullscreen={onToggleFullscreen}
        onMenuOpenChange={setMenuOpen}
      />

      <SubtitleSettingsModal
        isOpen={subtitleSettingsOpen}
        onClose={() => setSubtitleSettingsOpen(false)}
        subtitles={subtitles.options}
        selectedSubtitleId={subtitles.selectedId}
        onSelectSubtitle={handleSelectSubtitle}
        audioTracks={engine.audioTracks}
        selectedAudioTrackId={engine.audioTrack}
        onSelectAudioTrack={engine.selectAudioTrack}
        category={category}
        onToggleCategory={onToggleCategory}
        container={container}
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
