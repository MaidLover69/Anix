import React from 'react';
import { Check, ListPlus } from 'lucide-react';
import clsx from 'clsx';
import { ShimmerImage } from './ShimmerImage';
import { Button } from './ui/Button';
import { distinctEpisodeTitle } from '../hooks/watchProgress';
import type { EpisodeItem as EpisodeItemType } from '../types';

interface EpisodeItemProps {
  episode: EpisodeItemType;
  isWatched?: boolean;
  progressPercent?: number; // 0..100
  isFiller?: boolean;
  viewMode?: 'compact' | 'list' | 'grid';
  onSelect: (ep: EpisodeItemType) => void;
  onAddToPlaylist?: (ep: EpisodeItemType) => void;
}

// Row actions appear on hover and whenever focus is inside the row.
const REVEAL_CLASS =
  'opacity-0 transition-opacity focus-visible:opacity-100 group-focus-within:opacity-100 group-hover:opacity-100';

const FillerBadge: React.FC = () => (
  <span className="flex-shrink-0 rounded-sm bg-state-warning/15 px-1.5 py-0.5 text-[11px] font-semibold leading-[14px] text-state-warning">
    Filler
  </span>
);

const WatchedMark: React.FC<{ className?: string }> = ({ className }) => (
  <span
    className={clsx('flex flex-shrink-0 items-center gap-1 text-xs text-state-success', className)}
  >
    <Check aria-hidden="true" className="h-3.5 w-3.5" />
    Watched
  </span>
);

const ProgressBar: React.FC<{ percent: number }> = ({ percent }) => (
  <div
    role="progressbar"
    aria-label="Watch progress"
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={Math.round(percent)}
    className="absolute bottom-0 left-0 right-0 h-1 bg-white/20"
  >
    <div className="h-full bg-accent-500" style={{ width: `${percent}%` }} />
  </div>
);

const Thumbnail: React.FC<{ episode: EpisodeItemType; percent: number; className?: string }> = ({
  episode,
  percent,
  className,
}) => (
  <div className={clsx('relative aspect-video flex-shrink-0 overflow-hidden rounded-md bg-bg-elevated', className)}>
    {episode.image ? (
      <ShimmerImage src={episode.image} alt="" className="h-full w-full" />
    ) : (
      // No still for this episode: a number plate, so the tile reads as designed rather than
      // as an image that failed to load.
      <div
        aria-hidden="true"
        className="flex h-full w-full items-center justify-center rounded-md bg-gradient-to-br from-white/[0.07] to-transparent ring-1 ring-inset ring-white/[0.06]"
      >
        <span className="text-xl font-semibold tabular-nums text-text-disabled transition-colors group-hover:text-text-secondary">
          {episode.number}
        </span>
      </div>
    )}
    {percent > 0 && <ProgressBar percent={percent} />}
  </div>
);

export const EpisodeItem: React.FC<EpisodeItemProps> = ({
  episode,
  isWatched = false,
  progressPercent = 0,
  isFiller = false,
  viewMode = 'list',
  onSelect,
  onAddToPlaylist,
}) => {
  const title = distinctEpisodeTitle(episode.number, episode.title);
  const filler = isFiller || episode.filler;
  const percent = isWatched ? 0 : Math.min(100, Math.max(0, progressPercent));
  const playLabel = `Play episode ${episode.number}${title ? `: ${title}` : ''}`;

  // The row is a container with two sibling controls (play, add to playlist) so both are
  // real buttons that the keyboard can reach.
  const rowClass =
    'group relative flex rounded-md transition-colors hover:bg-white/5 focus-within:bg-white/5';

  const renderPlaylistButton = (className?: string) =>
    onAddToPlaylist && (
      <Button
        icon
        variant="ghost"
        size="sm"
        aria-label={`Add episode ${episode.number} to a playlist`}
        title="Add to playlist"
        onClick={() => onAddToPlaylist(episode)}
        className={clsx(REVEAL_CLASS, className)}
      >
        <ListPlus aria-hidden="true" className="h-4 w-4" />
      </Button>
    );

  if (viewMode === 'compact') {
    return (
      <div className={clsx(rowClass, 'items-center gap-2 pr-1')}>
        <button
          type="button"
          onClick={() => onSelect(episode)}
          aria-label={playLabel}
          className="flex h-9 min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-md px-3 text-left"
        >
          <span className="w-10 flex-shrink-0 text-[13px] font-medium tabular-nums text-text-muted">
            {episode.number}
          </span>
          <span
            className={clsx(
              'min-w-0 flex-1 truncate text-[13px]',
              isWatched ? 'text-text-muted' : 'text-text-primary'
            )}
          >
            {title || `Episode ${episode.number}`}
          </span>
          {percent > 0 && (
            <span className="flex-shrink-0 text-xs tabular-nums text-accent-300">
              {Math.round(percent)}%
            </span>
          )}
          {filler && <FillerBadge />}
          {isWatched && <WatchedMark />}
        </button>
        {renderPlaylistButton()}
      </div>
    );
  }

  if (viewMode === 'grid') {
    return (
      <div className={clsx(rowClass, 'flex-col p-2')}>
        <button
          type="button"
          onClick={() => onSelect(episode)}
          aria-label={playLabel}
          className="flex cursor-pointer flex-col rounded-md text-left"
        >
          <Thumbnail episode={episode} percent={percent} className="w-full" />
          <span className="mt-2 flex items-center gap-2">
            <span
              className={clsx(
                'min-w-0 flex-1 truncate text-[13px] font-medium',
                isWatched ? 'text-text-muted' : 'text-text-primary'
              )}
            >
              Episode {episode.number}
            </span>
            {filler && <FillerBadge />}
          </span>
          {title && <span className="mt-0.5 truncate text-xs text-text-muted">{title}</span>}
          {isWatched && <WatchedMark className="mt-1" />}
        </button>
        {renderPlaylistButton('absolute right-3 top-3 bg-black/70')}
      </div>
    );
  }

  return (
    <div className={clsx(rowClass, 'items-center gap-1 p-2')}>
      <button
        type="button"
        onClick={() => onSelect(episode)}
        aria-label={playLabel}
        className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-md text-left"
      >
        <Thumbnail episode={episode} percent={percent} className="w-28" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="flex items-center gap-2">
            <span
              className={clsx(
                'truncate text-[13px] font-medium',
                isWatched ? 'text-text-muted' : 'text-text-primary'
              )}
            >
              Episode {episode.number}
            </span>
            {filler && <FillerBadge />}
          </span>
          {title && <span className="mt-0.5 truncate text-xs text-text-muted">{title}</span>}
          {isWatched ? (
            <WatchedMark className="mt-1" />
          ) : (
            episode.synopsis && (
              <span className="mt-1 text-xs text-text-muted line-clamp-1">{episode.synopsis}</span>
            )
          )}
        </span>
      </button>
      {renderPlaylistButton()}
    </div>
  );
};
