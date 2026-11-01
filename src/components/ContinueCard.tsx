/** A wide "Continue watching" card on Home: the cover as an episode still, a progress ring round
 *  the play button, and a thin bar along the bottom. The picture resumes the episode, the title
 *  opens the anime page, and right-click (or Shift+F10) opens the poster menu. */
import React, { useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { Play } from 'lucide-react';
import { CoverThumb } from './library/CoverThumb';
import { PosterContextMenu } from './PosterContextMenu';
import { categoryLabel, episodeLabel, watchedPercent } from './library/format';
import { distinctEpisodeTitle, isHistoryEntryFinished, watchPath } from '../hooks/watchProgress';
import { useWatchlist } from '../hooks/useLibrary';
import { t } from '../i18n';
import type { HistoryEntry } from '../types';

export const ContinueCard: React.FC<{ entry: HistoryEntry; className?: string }> = ({ entry, className = '' }) => {
  const linkRef = useRef<HTMLAnchorElement>(null);
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const { data: watchlist = [] } = useWatchlist();
  const percent = watchedPercent(entry.position_ms, entry.duration_ms) ?? 0;
  const episode = `${episodeLabel(entry.episode_number)} · ${categoryLabel(entry.category)}`;
  const episodeTitle = distinctEpisodeTitle(entry.episode_number, entry.episode_title);
  // A finished episode goes to the anime page, which knows the next released episode (as in Library › History).
  const finished = isHistoryEntryFinished(entry);

  const openMenu = (event: React.MouseEvent | React.KeyboardEvent) => {
    event.preventDefault();
    const rect = linkRef.current?.getBoundingClientRect();
    setMenuAt(
      'clientX' in event && (event.clientX !== 0 || event.clientY !== 0)
        ? { x: event.clientX, y: event.clientY }
        : { x: (rect?.left ?? 0) + 8, y: (rect?.top ?? 0) + 8 }
    );
  };

  return (
    <div className={`ui-continue-card flex flex-shrink-0 snap-start flex-col gap-2 ${className}`} onContextMenu={openMenu}>
      <Link
        ref={linkRef}
        to={
          finished
            ? `/detail/${entry.anilist_id}`
            : watchPath(entry.anilist_id, entry.episode_number, entry.category, entry.provider?.trim())
        }
        draggable={false}
        aria-haspopup="menu"
        aria-label={
          finished
            ? t('historyTab.finishedLabel', { title: entry.title, episode })
            : t('historyTab.continueLabel', { title: entry.title, episode })
        }
        onKeyDown={(e) => {
          if ((e.key === 'F10' && e.shiftKey) || e.key === 'ContextMenu') openMenu(e);
        }}
        className="ui-continue-thumb ui-card relative block aspect-video overflow-hidden rounded-xl bg-bg-card"
      >
        <CoverThumb src={entry.cover} className="h-full w-full [&_img]:object-[50%_25%]" />
        {/* Text and the ring sit on the picture in every theme, so they use fixed light-on-dark colours. */}
        <span aria-hidden="true" className="absolute inset-0 bg-gradient-to-t from-black/75 via-black/10 to-transparent" />
        <span aria-hidden="true" className="ui-progress-ring absolute inset-0 m-auto grid h-14 w-14 place-items-center text-white">
          <svg viewBox="0 0 36 36" className="absolute inset-0 -rotate-90">
            <circle cx="18" cy="18" r="15.9" pathLength={100} className="fill-none stroke-white/25 [stroke-width:3.5]" />
            <circle
              cx="18"
              cy="18"
              r="15.9"
              pathLength={100}
              strokeDasharray="100"
              style={{ '--ring-offset': 100 - percent } as React.CSSProperties}
              className="ui-progress-ring-fg fill-none stroke-accent-500 [stroke-linecap:round] [stroke-width:3.5]"
            />
          </svg>
          <span className="grid h-10 w-10 place-items-center rounded-full bg-black/55">
            <Play className="ml-0.5 h-4 w-4 fill-current" />
          </span>
        </span>
        <span aria-hidden="true" className="absolute inset-x-3 bottom-2 flex justify-between gap-2 text-xs font-bold text-white">
          <span className="truncate">{episodeLabel(entry.episode_number)}</span>
          {percent > 0 && <span className="flex-shrink-0 tabular-nums">{t('historyTab.watchedPercent', { percent })}</span>}
        </span>
        <span aria-hidden="true" className="ui-hp absolute inset-x-0 bottom-0 h-[3px] bg-white/20">
          <span className="ui-hp-fill block h-full bg-accent-500" style={{ width: `${percent}%` }} />
        </span>
      </Link>
      <Link
        to={`/detail/${entry.anilist_id}`}
        draggable={false}
        title={entry.title}
        className="min-w-0 truncate rounded-sm text-[13px] font-semibold text-text-primary hover:underline"
      >
        <bdi>{entry.title}</bdi>
      </Link>
      {episodeTitle && <span className="-mt-1.5 truncate text-xs text-text-muted">{episodeTitle}</span>}

      {menuAt && (
        <PosterContextMenu
          target={{ id: entry.anilist_id, title: entry.title, cover: entry.cover, format: null, averageScore: null }}
          status={watchlist.find((w) => w.anilist_id === entry.anilist_id)?.status ?? null}
          at={menuAt}
          onClose={() => setMenuAt(null)}
          returnFocusTo={linkRef.current}
        />
      )}
    </div>
  );
};
