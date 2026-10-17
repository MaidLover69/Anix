import React from 'react';
import type { RefObject } from 'react';
import {
  AudioLines,
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
import { PlayerMenu } from './PlayerMenu';
import type { PlayerMenuOption } from './PlayerMenu';
import { PlayerTimeline } from './PlayerTimeline';
import { Slider } from './Slider';

export type AudioMode = 'normal' | 'boost';
export const AUDIO_MODES: readonly AudioMode[] = ['normal', 'boost'];

const AUDIO_OPTIONS: PlayerMenuOption[] = [
  { value: 'normal', label: 'Normal audio' },
  { value: 'boost', label: 'Dialogue boost (night mode)' },
];

const SPEED_OPTIONS: PlayerMenuOption[] = [
  { value: '0.5', label: '0.5×' },
  { value: '0.75', label: '0.75×' },
  { value: '1', label: 'Normal' },
  { value: '1.25', label: '1.25×' },
  { value: '1.5', label: '1.5×' },
  { value: '2', label: '2×' },
  { value: '3', label: '3×' },
  { value: '4', label: '4×' },
];

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

  rate: number;
  onRateChange: (rate: number) => void;

  /** Empty unless the stream offers a choice. */
  qualityOptions: PlayerMenuOption[];
  quality: string;
  onQualityChange: (value: string) => void;

  subtitlesOn: boolean;
  onOpenSubtitles: () => void;
  onOpenEpisodes: () => void;
  queuedCount: number;
  onOpenServers: () => void;

  isFullscreen: boolean;
  onToggleFullscreen: () => void;

  audioMode: AudioMode;
  onAudioModeChange: (mode: AudioMode) => void;
  onToggleMiniPlayer: () => void;
  /** Absent when the webview has no picture-in-picture. */
  onPictureInPicture?: () => void;

  onMenuOpenChange: (open: boolean) => void;
}

function rateLabel(rate: number): string {
  return `${Number(rate.toFixed(2))}×`;
}

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
  rate,
  onRateChange,
  qualityOptions,
  quality,
  onQualityChange,
  subtitlesOn,
  onOpenSubtitles,
  onOpenEpisodes,
  queuedCount,
  onOpenServers,
  isFullscreen,
  onToggleFullscreen,
  audioMode,
  onAudioModeChange,
  onToggleMiniPlayer,
  onPictureInPicture,
  onMenuOpenChange,
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
        className={`flex flex-col gap-1 px-4 pb-3 ${visible ? 'pointer-events-auto' : ''}`}
        onMouseDown={(e) => e.preventDefault()}
      >
        <PlayerTimeline videoRef={videoRef} duration={duration} onSeek={onSeek} />

        <div className="flex items-center justify-between gap-2">
          {/* Three groups on each side: episode and play, seek, volume | panels, playback options. */}
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1">
              <IconButton
                label="Previous episode"
                title={previous?.label ?? 'No previous episode'}
                disabled={!previous}
                onClick={previous?.onClick}
              >
                <SkipBack aria-hidden="true" className="h-4 w-4 fill-current" />
              </IconButton>
              <IconButton
                label={playing ? 'Pause' : 'Play'}
                title={playing ? 'Pause (Space)' : 'Play (Space)'}
                onClick={onTogglePlay}
              >
                {playing ? (
                  <Pause aria-hidden="true" className="h-5 w-5 fill-current" />
                ) : (
                  <Play aria-hidden="true" className="h-5 w-5 fill-current" />
                )}
              </IconButton>
              <IconButton
                label="Next episode"
                title={next?.label ?? 'No next episode'}
                disabled={!next}
                onClick={next?.onClick}
              >
                <SkipForward aria-hidden="true" className="h-4 w-4 fill-current" />
              </IconButton>
            </div>

            <div className="flex items-center gap-1">
              <IconButton label="Back 10 seconds" title="Back 10 seconds (J)" onClick={() => onSeekBy(-10)}>
                <RotateCcw aria-hidden="true" className="h-4 w-4" />
              </IconButton>
              <IconButton
                label="Forward 10 seconds"
                title="Forward 10 seconds (L)"
                onClick={() => onSeekBy(10)}
              >
                <RotateCw aria-hidden="true" className="h-4 w-4" />
              </IconButton>
            </div>

            <div className="flex items-center gap-1">
              <IconButton
                label={silent ? 'Unmute' : 'Mute'}
                title={silent ? 'Unmute (M)' : 'Mute (M)'}
                onClick={onToggleMute}
              >
                <VolumeIcon aria-hidden="true" className="h-4 w-4" />
              </IconButton>
              <Slider
                className="w-20"
                label="Volume"
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
              <IconButton
                label="Subtitles and audio"
                active={subtitlesOn}
                aria-pressed={subtitlesOn}
                onClick={onOpenSubtitles}
              >
                <Captions aria-hidden="true" className="h-4 w-4" />
              </IconButton>
              <IconButton
                label="Episodes"
                title={queuedCount > 0 ? `Episodes (${queuedCount} queued)` : 'Episodes'}
                onClick={onOpenEpisodes}
              >
                <ListVideo aria-hidden="true" className="h-4 w-4" />
                {queuedCount > 0 && <span className="ml-1 text-xs">{queuedCount}</span>}
              </IconButton>
              <IconButton label="Change server" onClick={onOpenServers}>
                <Server aria-hidden="true" className="h-4 w-4" />
              </IconButton>
            </div>

            <div className="flex items-center gap-1">
              <PlayerMenu
                label="Audio"
                value={audioMode}
                options={AUDIO_OPTIONS}
                onChange={(value) => onAudioModeChange(value === 'boost' ? 'boost' : 'normal')}
                onOpenChange={onMenuOpenChange}
              >
                <AudioLines
                  aria-hidden="true"
                  className={`h-4 w-4 ${audioMode === 'boost' ? 'text-accent-300' : ''}`}
                />
              </PlayerMenu>
              <PlayerMenu
                label="Playback speed"
                value={String(rate)}
                options={SPEED_OPTIONS}
                onChange={(value) => onRateChange(Number(value))}
                onOpenChange={onMenuOpenChange}
              >
                {rateLabel(rate)}
              </PlayerMenu>
              {qualityOptions.length > 0 && (
                <PlayerMenu
                  label="Quality"
                  value={quality}
                  options={qualityOptions}
                  onChange={onQualityChange}
                  onOpenChange={onMenuOpenChange}
                >
                  <Settings aria-hidden="true" className="h-4 w-4" />
                </PlayerMenu>
              )}
              {onPictureInPicture && (
                <IconButton label="Picture in picture" onClick={onPictureInPicture}>
                  <PictureInPicture2 aria-hidden="true" className="h-4 w-4" />
                </IconButton>
              )}
              <IconButton
                label="Mini-player"
                title="Mini-player: a small window that stays on top (I)"
                onClick={onToggleMiniPlayer}
              >
                <PictureInPicture aria-hidden="true" className="h-4 w-4" />
              </IconButton>
              <IconButton
                label={isFullscreen ? 'Exit fullscreen' : 'Fullscreen'}
                title={isFullscreen ? 'Exit fullscreen (F)' : 'Fullscreen (F)'}
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
