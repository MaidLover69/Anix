import React, { useEffect, useState } from 'react';
import { Bookmark, Play } from 'lucide-react';
import clsx from 'clsx';
import { ShimmerImage } from './ShimmerImage';
import { Button } from './ui/Button';
import { SegmentedControl } from './ui/SegmentedControl';

type Category = 'sub' | 'dub';

interface DetailStickyBarProps {
  /** The hero; the bar shows once it has scrolled out of view above. */
  anchorRef: React.RefObject<HTMLElement>;
  title: string;
  cover: string | null;
  inWatchlist: boolean;
  watchlistPending?: boolean;
  onToggleWatchlist: () => void;
  playLabel: string;
  playDisabled?: boolean;
  onPlay: () => void;
  category: Category;
  dubAvailable: boolean;
  onCategoryChange: (category: Category) => void;
}

/** True once `anchor` has scrolled up out of its scroll container (the app's <main>). */
function useScrolledPast(anchorRef: React.RefObject<HTMLElement>): boolean {
  const [past, setPast] = useState(false);
  useEffect(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const root = anchor.closest('main');
    const observer = new IntersectionObserver(
      ([entry]) => {
        const top = entry.rootBounds?.top ?? 0;
        setPast(!entry.isIntersecting && entry.boundingClientRect.bottom <= top + 1);
      },
      { root, threshold: 0, rootMargin: '-56px 0px 0px 0px' }
    );
    observer.observe(anchor);
    return () => observer.disconnect();
  }, [anchorRef]);
  return past;
}

/**
 * A slim frosted bar pinned to the top of the detail page while the hero is out of view: the
 * title, the watchlist toggle, the main play button and the audio switch. It is a sticky,
 * zero-height row of the page, so it takes no space and never covers the hero.
 */
export const DetailStickyBar: React.FC<DetailStickyBarProps> = ({
  anchorRef,
  title,
  cover,
  inWatchlist,
  watchlistPending,
  onToggleWatchlist,
  playLabel,
  playDisabled,
  onPlay,
  category,
  dubAvailable,
  onCategoryChange,
}) => {
  const visible = useScrolledPast(anchorRef);

  return (
    // -mb-8 cancels the page's gap-8, so the row adds no space.
    <div className="sticky top-0 z-40 -mb-8 h-0">
      {visible && (
        <div
          role="region"
          aria-label={`${title} actions`}
          className={clsx(
            'ui-frost ui-bar-in absolute inset-x-0 top-2 flex h-14 items-center gap-3 rounded-xl border border-border px-3 shadow-pop'
          )}
        >
          <ShimmerImage src={cover} alt="" className="aspect-[2/3] h-10 flex-shrink-0 rounded-md bg-bg-elevated" />
          <span title={title} className="min-w-0 flex-1 truncate text-sm font-semibold text-text-primary">
            {title}
          </span>
          <SegmentedControl
            aria-label="Audio"
            size="sm"
            value={category}
            onChange={onCategoryChange}
            options={[
              { value: 'sub', label: 'Sub' },
              { value: 'dub', label: 'Dub', disabled: !dubAvailable, title: dubAvailable ? undefined : 'No dub listed' },
            ]}
          />
          <Button
            icon
            size="sm"
            variant={inWatchlist ? 'secondary' : 'neutral'}
            aria-label={inWatchlist ? 'Remove from watchlist' : 'Add to watchlist'}
            title={inWatchlist ? 'In watchlist' : 'Add to watchlist'}
            aria-pressed={inWatchlist}
            disabled={watchlistPending}
            onClick={onToggleWatchlist}
          >
            <Bookmark aria-hidden="true" className={clsx('h-4 w-4', inWatchlist && 'fill-current')} />
          </Button>
          <Button variant="primary" size="sm" onClick={onPlay} disabled={playDisabled}>
            <Play aria-hidden="true" className="h-4 w-4 fill-current" />
            {playLabel}
          </Button>
        </div>
      )}
    </div>
  );
};
