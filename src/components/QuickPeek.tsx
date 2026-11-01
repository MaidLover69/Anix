import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Info, Play } from 'lucide-react';
import { create } from 'zustand';
import { Button } from './ui/Button';
import { useAnimeDetails } from '../hooks/useAniList';
import { useContinueWatching } from '../hooks/useLibrary';
import { useSettings, AUTO_PROVIDER } from '../hooks/useSettings';
import { getContinueTarget, releasedEpisodeCount, watchPath } from '../hooks/watchProgress';
import { mediaFormatLabel } from './library/format';
import { t } from '../i18n';
import { Meme, useHighModule } from './meme/Meme';
import type { AnimeDetails, Media } from '../types';

/** Hover (or keyboard focus) this long before a card's quick look opens. */
export const PEEK_OPEN_DELAY_MS = 400;
/** Grace period for moving the pointer from a card onto its quick look. */
const PEEK_CLOSE_DELAY_MS = 150;
/** Right after one closes, the next card opens at once instead of waiting again. */
const PEEK_WARM_MS = 400;

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
  id: string;
}

/**
 * The quick look itself. It is rendered inside the card, over the bottom of the poster, so
 * Tab reaches its buttons right after the card's own and it never covers the rest of a rail.
 */
export const QuickPeekCard: React.FC<QuickPeekCardProps> = ({ media, title, id }) => {
  const navigate = useNavigate();

  // Only fetched when the card's list data lacks something; cached like the detail page's.
  const detailsQuery = useAnimeDetails(needsDetails(media) ? media.id : null);
  const details = detailsQuery.data as AnimeDetails | null | undefined;
  const { data: history = [] } = useContinueWatching();
  const { data: settings } = useSettings();
  const high = useHighModule();

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

  const now = Math.floor(Date.now() / 1000);

  return (
    <div className="pointer-events-none absolute inset-x-0 top-0 flex aspect-[2/3] flex-col justify-end">
      <div
        id={id}
        role="group"
        aria-label={t('quickPeek.quickLook', { title })}
        className="ui-menu ui-peek-up pointer-events-auto m-1.5 flex flex-col gap-1.5 rounded-lg border border-border-strong bg-bg-elevated/95 p-2.5 text-start shadow-pop select-none"
      >
        {high && (
          <span className="meme-peek-tag self-start rounded-sm px-1.5 text-[0.6875rem] font-semibold">
            <Meme spot="peek" plain={t('quickPeek.quickLook', { title })} />
          </span>
        )}
        <p className="text-[0.8125rem] font-semibold leading-[1.125rem] text-text-primary line-clamp-2">{title}</p>
        {meta && <p className="text-[0.6875rem] leading-4 tabular-nums text-text-muted">{meta}</p>}
        {next && next.airing_at > now - 3600 && (
          <p className="text-[0.6875rem] font-medium leading-4 text-accent-300">
            {airsIn(next.episode, next.airing_at - now)}
          </p>
        )}
        {description ? (
          <p className="text-[0.6875rem] leading-4 text-text-secondary line-clamp-2">{description}</p>
        ) : (
          detailsQuery.isFetching && <div aria-hidden="true" className="h-8 rounded-sm shimmer-placeholder" />
        )}
        {genres.length > 0 && (
          <p className="truncate text-[0.6875rem] leading-4 text-text-muted">{genres.slice(0, 3).join(' · ')}</p>
        )}
        <div className="mt-0.5 flex items-center gap-1.5">
          {canPlay && (
            <Button
              variant="primary"
              size="sm"
              className="min-w-0 flex-1"
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
              <Play aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0 fill-current" />
              <span className="truncate">
                {target ? t('quickPeek.continueEpisode', { episode: playEpisode }) : t('quickPeek.playFirstEpisode')}
              </span>
            </Button>
          )}
          <Link
            to={`/detail/${media.id}`}
            draggable={false}
            aria-label={t('common.details')}
            title={t('common.details')}
            className="ui-btn inline-flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md bg-ink/[0.08] text-text-primary transition-colors hover:bg-ink/[0.14]"
            data-variant="neutral"
          >
            <Info aria-hidden="true" className="h-3.5 w-3.5" />
          </Link>
        </div>
      </div>
    </div>
  );
};
