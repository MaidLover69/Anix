import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Info, Play } from 'lucide-react';
import { create } from 'zustand';
import { Button } from './ui/Button';
import { Chip } from './ui/Chip';
import { useAnimeDetails } from '../hooks/useAniList';
import { useContinueWatching } from '../hooks/useLibrary';
import { useSettings, AUTO_PROVIDER } from '../hooks/useSettings';
import { getContinueTarget, releasedEpisodeCount, watchPath } from '../hooks/watchProgress';
import { mediaFormatLabel } from './library/format';
import { t } from '../i18n';
import type { AnimeDetails, Media } from '../types';

/** Hover (or keyboard focus) this long before a card's quick look opens. */
export const PEEK_OPEN_DELAY_MS = 400;
/** Grace period for moving the pointer from a card onto its quick look. */
const PEEK_CLOSE_DELAY_MS = 150;
/** Right after one closes, the next card opens at once instead of waiting again. */
const PEEK_WARM_MS = 400;

const PEEK_WIDTH = 300;
const GAP = 8;
const TOP_LIMIT = 40; // below the title bar

interface PeekStore {
  /** The card whose quick look is open. One at a time, app-wide. */
  openKey: string | null;
  closedAt: number;
  open: (key: string) => void;
  close: (key: string) => void;
}

const usePeekStore = create<PeekStore>((set, get) => ({
  openKey: null,
  closedAt: 0,
  open: (key) => set({ openKey: key }),
  close: (key) => {
    if (get().openKey === key) set({ openKey: null, closedAt: Date.now() });
  },
}));

let nextPeekId = 0;

/**
 * Open/close behaviour of one card's quick look: a delayed open on hover or keyboard focus,
 * a short grace period on leaving, and no waiting when moving straight from one open card
 * to the next (so moving along a rail never flickers).
 */
export function useQuickPeek() {
  const [key] = useState(() => `peek-${++nextPeekId}`);
  const isOpen = usePeekStore((s) => s.openKey === key);
  const openTimer = useRef<number>();
  const closeTimer = useRef<number>();

  const clearTimers = () => {
    window.clearTimeout(openTimer.current);
    window.clearTimeout(closeTimer.current);
  };

  const requestOpen = useCallback(() => {
    window.clearTimeout(closeTimer.current);
    const state = usePeekStore.getState();
    if (state.openKey === key) return;
    const warm = state.openKey !== null || Date.now() - state.closedAt < PEEK_WARM_MS;
    window.clearTimeout(openTimer.current);
    if (warm) state.open(key);
    else openTimer.current = window.setTimeout(() => usePeekStore.getState().open(key), PEEK_OPEN_DELAY_MS);
  }, [key]);

  const requestClose = useCallback(() => {
    window.clearTimeout(openTimer.current);
    window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => usePeekStore.getState().close(key), PEEK_CLOSE_DELAY_MS);
  }, [key]);

  const closeNow = useCallback(() => {
    clearTimers();
    usePeekStore.getState().close(key);
  }, [key]);

  useEffect(
    () => () => {
      clearTimers();
      usePeekStore.getState().close(key);
    },
    [key]
  );

  return { isOpen, requestOpen, requestClose, closeNow };
}

function plainText(html: string | null | undefined): string {
  return (html ?? '')
    .replace(/<br\s*\/?>/gi, ' ')
    .replace(/<[^>]*>?/gm, '')
    .replace(/&quot;/g, '"')
    .replace(/&#039;|&apos;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim();
}

function airsIn(episode: number, seconds: number): string {
  if (seconds <= 0) return t('quickPeek.airsNow', { episode });
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return t('quickPeek.airsInDays', { episode, days, hours });
  if (hours > 0) return t('quickPeek.airsInHours', { episode, hours, minutes });
  return t('quickPeek.airsInMinutes', { episode, minutes: Math.max(1, minutes) });
}

/** What the card's own data lacks that a quick look shows, so details must be fetched. */
function needsDetails(media: Media): boolean {
  if (!media.description || !media.genres?.length || !media.status) return true;
  return media.status === 'RELEASING' && !media.next_airing_episode;
}

interface QuickPeekCardProps {
  media: Media;
  title: string;
  /** The card; the quick look is placed beside it. */
  anchorRef: React.RefObject<HTMLElement>;
  id: string;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
  onDismiss: () => void;
}

/**
 * The quick look itself. It is rendered inside the card (so Tab reaches its buttons right
 * after the card's own) but positioned fixed beside it, so rails that scroll sideways do not
 * clip it.
 */
export const QuickPeekCard: React.FC<QuickPeekCardProps> = ({
  media,
  title,
  anchorRef,
  id,
  onPointerEnter,
  onPointerLeave,
  onDismiss,
}) => {
  const navigate = useNavigate();
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);

  // Only fetched when the card's list data lacks something; cached like the detail page's.
  const detailsQuery = useAnimeDetails(needsDetails(media) ? media.id : null);
  const details = detailsQuery.data as AnimeDetails | null | undefined;
  const { data: history = [] } = useContinueWatching();
  const { data: settings } = useSettings();

  const description = plainText(media.description || details?.description);
  const format = media.format ?? details?.format ?? null;
  const episodes = media.episodes ?? details?.episodes ?? null;
  const status = media.status ?? details?.status ?? null;
  const genres = media.genres?.length ? media.genres : details?.genres ?? [];
  const seasonYear = media.season_year ?? details?.seasonYear ?? null;
  const score = media.average_score ?? details?.averageScore ?? null;
  const next =
    media.next_airing_episode ??
    (details?.nextAiringEpisode
      ? {
          episode: details.nextAiringEpisode.episode,
          airing_at: details.nextAiringEpisode.airingAt,
        }
      : null);

  const latest = history.find((h) => h.anilist_id === media.id) ?? null;
  const released = releasedEpisodeCount(status, episodes, next?.episode);
  const target = getContinueTarget({ latest, released });
  const canPlay = released === null || released > 0;
  const playEpisode = target?.episode ?? 1;

  const meta = [
    mediaFormatLabel(format) || null,
    episodes ? t('common.episodeCount', { count: episodes }) : null,
    seasonYear,
    score ? `${score}%` : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const place = useCallback(() => {
    const anchor = anchorRef.current;
    const el = ref.current;
    if (!anchor || !el) return;
    const rect = anchor.getBoundingClientRect();
    // Scrolled out of view (a rail or the page moved): nothing to point at any more.
    if (rect.bottom < TOP_LIMIT || rect.top > window.innerHeight || rect.right < 0 || rect.left > window.innerWidth) {
      onDismiss();
      return;
    }
    const height = el.offsetHeight;
    let left = rect.right + GAP;
    if (left + PEEK_WIDTH > window.innerWidth - GAP) left = rect.left - GAP - PEEK_WIDTH;
    left = Math.max(GAP, left);
    const top = Math.max(TOP_LIMIT, Math.min(rect.top, window.innerHeight - height - GAP));
    setPosition((prev) => (prev && prev.left === left && prev.top === top ? prev : { left, top }));
  }, [anchorRef, onDismiss]);

  useLayoutEffect(() => {
    place();
  });

  useEffect(() => {
    let frame = 0;
    const onMove = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(place);
    };
    window.addEventListener('scroll', onMove, true);
    window.addEventListener('resize', onMove);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener('scroll', onMove, true);
      window.removeEventListener('resize', onMove);
    };
  }, [place]);

  const now = Math.floor(Date.now() / 1000);

  return (
    <div
      ref={ref}
      id={id}
      role="group"
      aria-label={t('quickPeek.quickLook', { title })}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      style={{
        width: PEEK_WIDTH,
        left: position?.left ?? -9999,
        top: position?.top ?? 0,
        visibility: position ? 'visible' : 'hidden',
      }}
      className="ui-menu ui-pop-in fixed z-[60] flex flex-col gap-2 rounded-xl border border-border-strong bg-bg-elevated p-4 text-start shadow-pop select-none"
    >
      <p className="text-sm font-semibold leading-5 text-text-primary line-clamp-2">{title}</p>
      {meta && <p className="text-xs tabular-nums text-text-muted">{meta}</p>}
      {next && next.airing_at > now - 3600 && (
        <p className="text-xs font-medium text-accent-300">
          {airsIn(next.episode, next.airing_at - now)}
        </p>
      )}
      {description ? (
        <p className="text-xs leading-[18px] text-text-secondary line-clamp-2">{description}</p>
      ) : (
        detailsQuery.isFetching && <div aria-hidden="true" className="h-9 rounded-sm shimmer-placeholder" />
      )}
      {genres.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {genres.slice(0, 4).map((genre) => (
            <Chip key={genre} className="h-6 px-2 text-[11px]">
              {genre}
            </Chip>
          ))}
        </div>
      )}
      <div className="mt-1 flex items-center gap-2">
        {canPlay && (
          <Button
            variant="primary"
            size="sm"
            onClick={() =>
              navigate(
                watchPath(
                  media.id,
                  playEpisode,
                  target?.category ?? (settings?.prefer_dub ? 'dub' : 'sub'),
                  target?.provider ?? settings?.preferred_provider ?? AUTO_PROVIDER
                )
              )
            }
          >
            <Play aria-hidden="true" className="h-3.5 w-3.5 fill-current" />
            {target ? t('quickPeek.continueEpisode', { episode: playEpisode }) : t('quickPeek.playFirstEpisode')}
          </Button>
        )}
        <Link
          to={`/detail/${media.id}`}
          draggable={false}
          className="ui-btn inline-flex h-8 items-center gap-2 rounded-md bg-ink/[0.08] px-3 text-[13px] font-medium text-text-primary transition-colors hover:bg-ink/[0.14]"
          data-variant="neutral"
        >
          <Info aria-hidden="true" className="h-3.5 w-3.5" />
          {t('common.details')}
        </Link>
      </div>
    </div>
  );
};
