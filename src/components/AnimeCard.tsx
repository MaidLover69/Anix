import React, { useId, useRef, useState } from 'react';
import { Link } from 'react-router-dom';
import { Bookmark } from 'lucide-react';
import clsx from 'clsx';
import { ShimmerImage } from './ShimmerImage';
import type { Media } from '../types';
import { useWatchlist, useToggleWatchlist } from '../hooks/useLibrary';
import { QuickPeekCard, useQuickPeek } from './QuickPeek';
import { PosterContextMenu } from './PosterContextMenu';
import { t } from '../i18n';

interface AnimeCardProps {
  media: Media;
  className?: string;
  showProgress?: number; // 0..100
  style?: React.CSSProperties;
  /** The quick look on hover and keyboard focus (on by default). */
  peek?: boolean;
}

const BADGE_CLASS =
  'rounded-sm bg-black/70 px-1.5 py-0.5 text-[11px] font-semibold leading-[14px] text-white';

export const AnimeCard: React.FC<AnimeCardProps> = ({
  media,
  className = '',
  showProgress,
  style,
  peek = true,
}) => {
  const rootRef = useRef<HTMLDivElement>(null);
  const linkRef = useRef<HTMLAnchorElement>(null);
  const peekId = useId();
  const quickPeek = useQuickPeek();
  const peekOpen = peek && quickPeek.isOpen;
  // One shared query for every card on screen instead of a lookup per card.
  const { data: watchlist = [] } = useWatchlist();
  const listEntry = watchlist.find((entry) => entry.anilist_id === media.id);
  const inWatchlist = listEntry !== undefined;
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const toggleWatchlistMutation = useToggleWatchlist();

  const title =
    media.title.english ||
    media.title.user_preferred ||
    media.title.romaji ||
    t('animeCard.untitled');

  const coverUrl =
    media.cover_image.extra_large ||
    media.cover_image.large ||
    null;

  const handleWatchlistClick = () => {
    toggleWatchlistMutation.mutate({
      anilistId: media.id,
      title,
      cover: coverUrl,
      format: media.format,
      averageScore: media.average_score,
      inWatchlist,
    });
  };

  // Right-click opens the menu at the pointer; Shift+F10 and the menu key open it on the poster.
  const openMenu = (event: React.MouseEvent | React.KeyboardEvent) => {
    event.preventDefault();
    quickPeek.closeNow();
    const fromKeyboard = !('clientX' in event) || (event.clientX === 0 && event.clientY === 0);
    if (fromKeyboard) {
      const rect = linkRef.current?.getBoundingClientRect();
      setMenuAt({ x: (rect?.left ?? 0) + 8, y: (rect?.top ?? 0) + 8 });
    } else {
      setMenuAt({ x: event.clientX, y: event.clientY });
    }
  };

  const caption = [media.season_year, media.genres?.[0]].filter(Boolean).join(' · ');
  const format = media.format && media.format !== 'TV' ? media.format.replace(/_/g, ' ') : null;

  return (
    <div
      ref={rootRef}
      className={`group relative flex flex-col flex-shrink-0 select-none ${className}`}
      style={style}
      onContextMenu={openMenu}
      onPointerEnter={peek ? (e) => e.pointerType !== 'touch' && quickPeek.requestOpen() : undefined}
      onPointerLeave={peek ? quickPeek.requestClose : undefined}
      onPointerDown={
        peek
          ? (e) => {
              // Opening the card closes its quick look; a press inside the quick look does not.
              if (linkRef.current?.contains(e.target as Node)) quickPeek.closeNow();
            }
          : undefined
      }
      onFocus={
        peek
          ? (e) => {
              // Keyboard focus only; a click focuses the link too and must not open it.
              if (e.target instanceof HTMLElement && e.target.matches(':focus-visible')) quickPeek.requestOpen();
            }
          : undefined
      }
      onBlur={
        peek
          ? (e) => {
              if (!rootRef.current?.contains(e.relatedTarget as Node | null)) quickPeek.closeNow();
            }
          : undefined
      }
      onKeyDown={
        peekOpen
          ? (e) => {
              if (e.key !== 'Escape') return;
              e.stopPropagation();
              const inside = document.activeElement !== linkRef.current;
              quickPeek.closeNow();
              if (inside) linkRef.current?.focus();
            }
          : undefined
      }
    >
      <Link
        ref={linkRef}
        to={`/detail/${media.id}`}
        draggable={false}
        aria-describedby={peekOpen ? peekId : undefined}
        aria-haspopup="menu"
        onKeyDown={(e) => {
          if ((e.key === 'F10' && e.shiftKey) || e.key === 'ContextMenu') openMenu(e);
        }}
        className="flex flex-col rounded-xl"
      >
        <div className="relative w-full aspect-[2/3] rounded-xl overflow-hidden ui-card bg-bg-card">
          <ShimmerImage
            src={coverUrl}
            alt=""
            className="w-full h-full transition-transform duration-150 group-hover:scale-[1.04]"
          />

          {format && <span className={clsx(BADGE_CLASS, 'absolute start-2 top-2')}>{format}</span>}

          {media.average_score !== null && media.average_score !== undefined && (
            <span className={clsx(BADGE_CLASS, 'absolute end-2 top-2 tabular-nums')}>
              {media.average_score}%
            </span>
          )}

          {showProgress !== undefined && showProgress > 0 && (
            <div className="absolute bottom-0 start-0 end-0 h-1 bg-white/20">
              <div
                className="h-full bg-accent-500"
                style={{ width: `${Math.min(100, Math.max(0, showProgress))}%` }}
              />
            </div>
          )}
        </div>

        <h3 title={title} className="mt-2 text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
          {title}
        </h3>
        {caption && <span className="mt-0.5 truncate text-xs text-text-muted">{caption}</span>}
      </Link>

      {/* A sibling of the link, not a child: a button inside a link is invalid and unreachable by keyboard. */}
      <button
        type="button"
        onClick={handleWatchlistClick}
        aria-label={t(inWatchlist ? 'animeCard.removeFromWatchlist' : 'animeCard.addToWatchlist', { title })}
        aria-pressed={inWatchlist}
        className={clsx(
          'absolute start-2 flex h-8 w-8 items-center justify-center rounded-md bg-black/70 cursor-pointer transition-[opacity,background-color] hover:bg-black/90',
          format ? 'top-9' : 'top-2',
          // The chip is always black, so the saved state uses a fixed light colour in every theme.
          inWatchlist
            ? 'text-white'
            : 'text-white opacity-0 focus-visible:opacity-100 group-hover:opacity-100'
        )}
      >
        <Bookmark aria-hidden="true" className={clsx('h-4 w-4', inWatchlist && 'fill-current')} />
      </button>

      {menuAt && (
        <PosterContextMenu
          target={{
            id: media.id,
            title,
            cover: coverUrl,
            format: media.format,
            averageScore: media.average_score,
          }}
          status={listEntry?.status ?? null}
          at={menuAt}
          onClose={() => setMenuAt(null)}
          returnFocusTo={linkRef.current}
        />
      )}

      {peekOpen && (
        <QuickPeekCard
          id={peekId}
          media={media}
          title={title}
          anchorRef={rootRef}
          onPointerEnter={quickPeek.requestOpen}
          onPointerLeave={quickPeek.requestClose}
          onDismiss={quickPeek.closeNow}
        />
      )}
    </div>
  );
};
