import React from 'react';
import type { RefObject } from 'react';
import {
  Captions,
  ListVideo,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture,
  PictureInPicture2,
  Play,
  RotateCcw,
  RotateCw,
  Server,
  Settings,
  SkipBack,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from 'lucide-react';
import { IconButton } from './IconButton';
import { PlayerTimeline } from './PlayerTimeline';
import { Slider } from './Slider';
import { t } from '../../i18n';

export type AudioMode = 'normal' | 'boost';
export const AUDIO_MODES: readonly AudioMode[] = ['normal', 'boost'];

export interface EpisodeStep {
  /** Tooltip, for example "Next: Episode 6 (Shift+N)". */
  label: string;
  onClick: () => void;
}

interface PlayerControlsProps {
  videoRef: RefObject<HTMLVideoElement>;
  visible: boolean;

  playing: boolean;
  duration: number;
  onTogglePlay: () => void;
  onSeek: (time: number) => void;
  onSeekBy: (seconds: number) => void;

  /** Null at the first / last episode. */
  previous: EpisodeStep | null;
  next: EpisodeStep | null;

  volume: number;
  muted: boolean;
  onVolumeChange: (volume: number) => void;
  onToggleMute: () => void;

  subtitlesOn: boolean;
  onOpenSubtitles: () => void;
  onOpenEpisodes: () => void;
  queuedCount: number;
  onOpenServers: () => void;
  /** Speed, quality, screen, sound, upscaling and skipping. */
  onOpenSettings: () => void;

  isFullscreen: boolean;
  onToggleFullscreen: () => void;

  onToggleMiniPlayer: () => void;
  /** Absent when the webview has no picture-in-picture. */
  onPictureInPicture?: () => void;
}

/** A control-bar button with its name written next to the icon. */
const LabelledButton: React.FC<{
  label: string;
  title?: string;
  onClick: () => void;
  active?: boolean;
  children: React.ReactNode;
}> = ({ label, title, onClick, active, children }) => (
  <IconButton label={label} title={title} active={active} onClick={onClick} className="gap-1.5">
    {children}
    <span aria-hidden="true">{label}</span>
  </IconButton>
);

export const PlayerControls: React.FC<PlayerControlsProps> = ({
  videoRef,
  visible,
  playing,
  duration,
  onTogglePlay,
  onSeek,
  onSeekBy,
  previous,
  next,
  volume,
  muted,
  onVolumeChange,
  onToggleMute,
  subtitlesOn,
  onOpenSubtitles,
  onOpenEpisodes,
  queuedCount,
  onOpenServers,
  onOpenSettings,
  isFullscreen,
  onToggleFullscreen,
  onToggleMiniPlayer,
  onPictureInPicture,
}) => {
  const silent = muted || volume === 0;
  const shownVolume = silent ? 0 : volume;
  const VolumeIcon = silent ? VolumeX : volume < 0.5 ? Volume1 : Volume2;

  return (
    <div
      // The scrim is already dense behind the seek bar, so the controls stay readable over a bright frame.
      className={`pointer-events-none absolute inset-x-0 bottom-0 z-20 bg-gradient-to-t from-black/90 via-black/60 to-transparent pt-16 transition-opacity duration-200 ${
        visible ? 'opacity-100' : 'opacity-0'
      }`}
    >
      {/* A mouse press must not move focus onto a control, or Space would press it again. */}
      <div
        data-player-bar="bottom"
        className={`flex flex-col gap-1 px-4 pb-3 ${visible ? 'pointer-events-auto' : ''}`}
        onMouseDown={(e) => e.preventDefault()}
      >
        <PlayerTimeline videoRef={videoRef} duration={duration} onSeek={onSeek} />

        <div className="flex items-center justify-between gap-2">
          {/* Three groups on each side: episode and play, seek, volume | panels, playback options. */}
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1">
              <IconButton
                label={t('playerControls.previousEpisode')}
                title={previous?.label ?? t('playerControls.noPreviousEpisode')}
                disabled={!previous}
                onClick={previous?.onClick}
              >
                <SkipBack aria-hidden="true" className="h-4 w-4 fill-current" />
              </IconButton>
              <IconButton
                label={playing ? t('common.pause') : t('common.play')}
                title={playing ? t('playerControls.pauseWithKey') : t('playerControls.playWithKey')}
                onClick={onTogglePlay}
              >
                {playing ? (
                  <Pause aria-hidden="true" className="h-5 w-5 fill-current" />
                ) : (
                  <Play aria-hidden="true" className="h-5 w-5 fill-current" />
                )}
              </IconButton>
              <IconButton
                label={t('playerControls.nextEpisode')}
                title={next?.label ?? t('playerControls.noNextEpisode')}
                disabled={!next}
                onClick={next?.onClick}
              >
                <SkipForward aria-hidden="true" className="h-4 w-4 fill-current" />
              </IconButton>
            </div>

            <div className="flex items-center gap-1">
              <IconButton
                label={t('playerControls.back10')}
                title={t('playerControls.back10WithKey')}
                onClick={() => onSeekBy(-10)}
              >
                <RotateCcw aria-hidden="true" className="h-4 w-4" />
              </IconButton>
              <IconButton
                label={t('playerControls.forward10')}
                title={t('playerControls.forward10WithKey')}
                onClick={() => onSeekBy(10)}
              >
                <RotateCw aria-hidden="true" className="h-4 w-4" />
              </IconButton>
            </div>

            <div className="flex items-center gap-1">
              <IconButton
                label={silent ? t('playerControls.unmute') : t('playerControls.mute')}
                title={silent ? t('playerControls.unmuteWithKey') : t('playerControls.muteWithKey')}
                onClick={onToggleMute}
              >
                <VolumeIcon aria-hidden="true" className="h-4 w-4" />
              </IconButton>
              <Slider
                className="w-20"
                label={t('playerControls.volume')}
                value={shownVolume}
                max={1}
                keyStep={0.05}
                valueText={`${Math.round(shownVolume * 100)}%`}
                onChange={onVolumeChange}
              />
            </div>
          </div>

          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1">
              <LabelledButton label={t('playerControls.server')} title={t('playerControls.changeServer')} onClick={onOpenServers}>
                <Server aria-hidden="true" className="h-4 w-4" />
              </LabelledButton>
              <LabelledButton
                label={t('common.episodes')}
                title={queuedCount > 0 ? t('playerControls.episodesQueued', { count: queuedCount }) : t('common.episodes')}
                onClick={onOpenEpisodes}
              >
                <ListVideo aria-hidden="true" className="h-4 w-4" />
                {queuedCount > 0 && <span className="text-xs">{queuedCount}</span>}
              </LabelledButton>
              <LabelledButton
                label={t('playerControls.subtitles')}
                title={t('playerControls.subtitlesAndAudio')}
                active={subtitlesOn}
                onClick={onOpenSubtitles}
              >
                <Captions aria-hidden="true" className="h-4 w-4" />
              </LabelledButton>
              <LabelledButton label={t('common.settings')} title={t('playerControls.settingsHint')} onClick={onOpenSettings}>
                <Settings aria-hidden="true" className="h-4 w-4" />
              </LabelledButton>
            </div>

            <div className="flex items-center gap-1">
              {onPictureInPicture && (
                <IconButton label={t('playerControls.pictureInPicture')} onClick={onPictureInPicture}>
                  <PictureInPicture2 aria-hidden="true" className="h-4 w-4" />
                </IconButton>
              )}
              <IconButton
                label={t('playerControls.miniPlayer')}
                title={t('playerControls.miniPlayerHint')}
                onClick={onToggleMiniPlayer}
              >
                <PictureInPicture aria-hidden="true" className="h-4 w-4" />
              </IconButton>
              <IconButton
                label={isFullscreen ? t('playerControls.exitFullscreen') : t('playerControls.fullscreen')}
                title={isFullscreen ? t('playerControls.exitFullscreenWithKey') : t('playerControls.fullscreenWithKey')}
                onClick={onToggleFullscreen}
              >
                {isFullscreen ? (
                  <Minimize aria-hidden="true" className="h-4 w-4" />
                ) : (
                  <Maximize aria-hidden="true" className="h-4 w-4" />
                )}
              </IconButton>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
