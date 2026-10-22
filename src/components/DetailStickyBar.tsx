import React, { useEffect, useId, useState } from 'react';
import { Play } from 'lucide-react';
import clsx from 'clsx';
import { ShimmerImage } from './ShimmerImage';
import { Button } from './ui/Button';
import { SegmentedControl } from './ui/SegmentedControl';
import { errorMessage } from './ui/ErrorState';
import { CustomSelect } from './CustomSelect';
import type { PosterMenuTarget } from './PosterContextMenu';
import { LIST_STATUS_OPTIONS, useListStatus, useSetListStatus, useWatchlist } from '../hooks/useLibrary';
import { useUIStore } from '../stores/uiStore';
import type { ListStatusValue } from '../types';
import { t } from '../i18n';

type Category = 'sub' | 'dub';

interface DetailStickyBarProps {
  /** The hero; the bar shows once it has scrolled out of view above. */
  anchorRef: React.RefObject<HTMLElement>;
  title: string;
  cover: string | null;
  /** The anime the status dropdown files. */
  listTarget: PosterMenuTarget;
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
 * title, the list status, the main play button and the audio switch. It is a sticky,
 * zero-height row of the page, so it takes no space and never covers the hero.
 */
export const DetailStickyBar: React.FC<DetailStickyBarProps> = ({
  anchorRef,
  title,
  cover,
  listTarget,
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
          aria-label={t('detailStickyBar.titleActions', { title })}
          className={clsx(
            'ui-frost ui-bar-in absolute inset-x-0 top-2 flex h-14 items-center gap-3 rounded-xl border border-border px-3 shadow-pop'
          )}
        >
          <ShimmerImage src={cover} alt="" className="aspect-[2/3] h-10 flex-shrink-0 rounded-md bg-bg-elevated" />
          <span title={title} className="min-w-0 flex-1 truncate text-sm font-semibold text-text-primary">
            {title}
          </span>
          <SegmentedControl
            aria-label={t('detailStickyBar.audio')}
            size="sm"
            value={category}
            onChange={onCategoryChange}
            options={[
              { value: 'sub', label: t('common.sub') },
              { value: 'dub', label: t('common.dub'), disabled: !dubAvailable, title: dubAvailable ? undefined : t('detailStickyBar.noDubListed') },
            ]}
          />
          <ListStatusSelect target={listTarget} size="sm" />
          <Button variant="primary" size="sm" onClick={onPlay} disabled={playDisabled}>
            <Play aria-hidden="true" className="h-4 w-4 fill-current" />
            {playLabel}
          </Button>
        </div>
      )}
    </div>
  );
};

const REMOVE = 'REMOVE';

/**
 * The list status of one anime as a dropdown: the connected tracker's status when it has the
 * anime, else the watchlist's. Choosing one files the anime (and sends it to the tracker);
 * "Remove from list" takes it off the watchlist (offered only when the tracker does not have it).
 */
export const ListStatusSelect: React.FC<{ target: PosterMenuTarget; size?: 'sm' | 'md'; className?: string }> = ({
  target,
  size = 'md',
  className,
}) => {
  const hintId = useId();
  const showToast = useUIStore((s) => s.showToast);
  const { data: watchlist = [] } = useWatchlist();
  const { data: tracked } = useListStatus(target.id);
  const setStatus = useSetListStatus();
  const local = watchlist.find((entry) => entry.anilist_id === target.id);
  // The tracker wins, except over a change made here that is still waiting to be sent.
  const fromTracker = tracked !== undefined && tracked.source !== 'local' && local?.synced !== false;
  const status: ListStatusValue | null = fromTracker ? tracked.status : (local?.status ?? tracked?.status ?? null);
  const waiting = local ? !local.synced : tracked?.synced === false;

  // Removing only clears the watchlist row, so it is not offered while the tracker has the anime.
  const options = [
    ...LIST_STATUS_OPTIONS,
    ...(status !== null && !fromTracker ? [{ value: REMOVE, label: t('detailStickyBar.removeFromList') }] : []),
  ];

  const handleChange = (value: string) => {
    const next = value === REMOVE ? null : (value as ListStatusValue);
    if (next === status) return;
    setStatus.mutate(
      {
        anilistId: target.id,
        title: target.title,
        cover: target.cover,
        format: target.format,
        averageScore: target.averageScore,
        status: next,
      },
      {
        onError: (err) => showToast({ type: 'error', title: t('detailStickyBar.couldNotUpdateList'), message: errorMessage(err) }),
      }
    );
  };

  return (
    <>
      <CustomSelect
        aria-label={t('detailStickyBar.listStatus')}
        aria-describedby={waiting ? hintId : undefined}
        className={clsx(size === 'sm' ? 'w-36' : 'w-44', className)}
        size={size}
        value={status ?? ''}
        placeholder={t('detailStickyBar.addToList')}
        onChange={handleChange}
        options={options}
        disabled={setStatus.isPending}
      />
      {waiting && (
        <span id={hintId} className="sr-only">
          {t('detailStickyBar.notSentYet')}
        </span>
      )}
    </>
  );
};
