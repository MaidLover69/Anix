import React from 'react';
import type { RefObject } from 'react';
import { Maximize2, Pause, Play, SkipForward } from 'lucide-react';
import { IconButton } from './IconButton';
import { PlayerTimeline } from './PlayerTimeline';
import type { EpisodeStep } from './PlayerControls';
import { t } from '../../i18n';

interface MiniPlayerChromeProps {
  videoRef: RefObject<HTMLVideoElement>;
  visible: boolean;
  title: string;
  playing: boolean;
  duration: number;
  onTogglePlay: () => void;
  onSeek: (time: number) => void;
  next: EpisodeStep | null;
  onExit: () => void;
}

/**
 * The controls of the always-on-top mini-player: a title strip that moves the window, Pause,
 * next and the way back to the full window in the middle, and the timeline. They show while the
 * mouse moves over the window or a control has keyboard focus.
 */
export const MiniPlayerChrome: React.FC<MiniPlayerChromeProps> = ({
  videoRef,
  visible,
  title,
  playing,
  duration,
  onTogglePlay,
  onSeek,
  next,
  onExit,
}) => (
  <div
    className={`pointer-events-none absolute inset-0 z-20 flex flex-col justify-between bg-gradient-to-b from-black/70 via-black/40 to-black/80 transition-opacity duration-200 focus-within:opacity-100 ${
      visible ? 'opacity-100' : 'opacity-0'
    }`}
  >
    <div
      data-tauri-drag-region
      className={`flex h-8 items-center px-3 cursor-grab ${visible ? 'pointer-events-auto' : ''}`}
    >
      <span data-tauri-drag-region className="min-w-0 flex-1 truncate text-xs font-medium text-white">
        {title}
      </span>
    </div>

    <div
      className={`flex items-center justify-center gap-1 ${visible ? 'pointer-events-auto' : ''}`}
      onMouseDown={(e) => e.preventDefault()}
    >
      <IconButton
        label={playing ? t('common.pause') : t('common.play')}
        title={playing ? t('playerControls.pauseWithKey') : t('playerControls.playWithKey')}
        onClick={onTogglePlay}
        className="h-10 min-w-10"
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
      <IconButton label={t('miniPlayerChrome.backToFullWindow')} onClick={onExit} className="gap-1.5 bg-white/10">
        <Maximize2 aria-hidden="true" className="h-4 w-4" />
        {/* The smallest mini window (320 px) keeps only the icon. */}
        <span aria-hidden="true" className="hidden whitespace-nowrap min-[400px]:inline">
          {t('miniPlayerChrome.backToFullWindow')}
        </span>
      </IconButton>
    </div>

    <div
      className={`px-2 pb-1.5 ${visible ? 'pointer-events-auto' : ''}`}
      onMouseDown={(e) => e.preventDefault()}
    >
      <PlayerTimeline videoRef={videoRef} duration={duration} onSeek={onSeek} />
    </div>
  </div>
);
