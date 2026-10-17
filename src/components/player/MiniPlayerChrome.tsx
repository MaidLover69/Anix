import React from 'react';
import type { RefObject } from 'react';
import { Maximize2, Pause, Play, SkipForward } from 'lucide-react';
import { IconButton } from './IconButton';
import { PlayerTimeline } from './PlayerTimeline';
import type { EpisodeStep } from './PlayerControls';

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
 * The controls of the always-on-top mini-player: a title strip that moves the window, play,
 * next, the timeline and the way back to the full window.
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
    className={`pointer-events-none absolute inset-0 z-20 flex flex-col justify-between bg-gradient-to-b from-black/70 via-transparent to-black/80 transition-opacity duration-200 ${
      visible ? 'opacity-100' : 'opacity-0'
    }`}
  >
    <div
      data-tauri-drag-region
      className={`flex h-9 items-center gap-1 pl-3 pr-1 cursor-grab ${visible ? 'pointer-events-auto' : ''}`}
    >
      <span data-tauri-drag-region className="min-w-0 flex-1 truncate text-xs font-medium text-white">
        {title}
      </span>
      <IconButton label="Back to the full window" onClick={onExit} onMouseDown={(e) => e.preventDefault()}>
        <Maximize2 aria-hidden="true" className="h-4 w-4" />
      </IconButton>
    </div>

    <div
      className={`flex flex-col gap-0.5 px-2 pb-1.5 ${visible ? 'pointer-events-auto' : ''}`}
      onMouseDown={(e) => e.preventDefault()}
    >
      <PlayerTimeline videoRef={videoRef} duration={duration} onSeek={onSeek} />
      <div className="flex items-center gap-1">
        <IconButton label={playing ? 'Pause' : 'Play'} title={playing ? 'Pause (Space)' : 'Play (Space)'} onClick={onTogglePlay}>
          {playing ? (
            <Pause aria-hidden="true" className="h-4 w-4 fill-current" />
          ) : (
            <Play aria-hidden="true" className="h-4 w-4 fill-current" />
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
    </div>
  </div>
);
