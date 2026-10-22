import React, { useRef } from 'react';
import { Link } from 'react-router-dom';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { AnimeCard } from './AnimeCard';
import { SkeletonCard } from './SkeletonCard';
import { Button } from './ui/Button';
import { distinctEpisodeTitle, historyProgressPercent } from '../hooks/watchProgress';
import { t } from '../i18n';
import type { Media, HistoryEntry } from '../types';

interface HorizontalRailProps {
  title: string;
  items?: Media[];
  historyItems?: HistoryEntry[];
  isLoading?: boolean;
  viewAllLink?: string;
}

const CARD_CLASS = 'w-44 flex-shrink-0 snap-start';

function historyToMedia(hist: HistoryEntry): Media {
  return {
    id: hist.anilist_id,
    id_mal: null,
    title: {
      english: hist.title,
      romaji: hist.title,
      native: null,
      user_preferred: hist.title,
    },
    cover_image: {
      extra_large: hist.cover,
      large: hist.cover,
      color: null,
    },
    banner_image: null,
    description: null,
    format: null,
    status: null,
    episodes: null,
    duration: null,
    season: null,
    season_year: null,
    average_score: null,
    mean_score: null,
    popularity: null,
    favourites: null,
    genres: [],
    is_adult: false,
  };
}

export const HorizontalRail: React.FC<HorizontalRailProps> = ({
  title,
  items,
  historyItems,
  isLoading = false,
  viewAllLink,
}) => {
  const scrollRef = useRef<HTMLDivElement>(null);

  const handleScroll = (direction: 'left' | 'right') => {
    const el = scrollRef.current;
    if (!el) return;
    const distance = Math.max(el.clientWidth - 176, 176);
    el.scrollBy({
      left: direction === 'left' ? -distance : distance,
      // An explicit behaviour overrides the CSS reduced-motion rule, so check it here.
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
    });
  };

  const hasContent = (items && items.length > 0) || (historyItems && historyItems.length > 0);
  if (!isLoading && !hasContent) return null;

  return (
    <section className="group/rail flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4">
        <h2 className="text-xl font-semibold text-text-primary">{title}</h2>

        <div className="flex items-center gap-1">
          <div dir="ltr" className="flex items-center gap-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover/rail:opacity-100">
            <Button icon variant="ghost" size="sm" aria-label={t('horizontalRail.scrollLeft', { title })} onClick={() => handleScroll('left')}>
              <ChevronLeft aria-hidden="true" className="h-4 w-4" />
            </Button>
            <Button icon variant="ghost" size="sm" aria-label={t('horizontalRail.scrollRight', { title })} onClick={() => handleScroll('right')}>
              <ChevronRight aria-hidden="true" className="h-4 w-4" />
            </Button>
          </div>
          {viewAllLink && (
            <Link
              to={viewAllLink}
              draggable={false}
              className="-me-3 flex h-8 items-center rounded-md px-3 text-[13px] font-medium text-text-secondary transition-colors hover:bg-ink/10 hover:text-text-primary"
            >
              {t('horizontalRail.viewAll')}
            </Link>
          )}
        </div>
      </div>

      <div
        ref={scrollRef}
        className="-mx-1 flex snap-x snap-mandatory scroll-px-1 items-start gap-4 overflow-x-auto px-1 py-1 no-scrollbar"
      >
        {isLoading && !hasContent
          ? Array.from({ length: 7 }).map((_, i) => <SkeletonCard key={i} className="w-44" />)
          : historyItems && historyItems.length > 0
          ? historyItems.map((hist) => {
              const episodeTitle = distinctEpisodeTitle(hist.episode_number, hist.episode_title);
              return (
                <div key={hist.anilist_id} className={`${CARD_CLASS} flex flex-col`}>
                  <AnimeCard
                    media={historyToMedia(hist)}
                    showProgress={historyProgressPercent(hist)}
                    className="w-full"
                  />
                  <span className="mt-0.5 truncate text-xs text-text-muted">
                    {t('common.episodeNumber', { number: hist.episode_number })}
                    {episodeTitle ? ` · ${episodeTitle}` : ''}
                  </span>
                </div>
              );
            })
          : items?.map((anime) => <AnimeCard key={anime.id} media={anime} className={CARD_CLASS} />)}
      </div>
    </section>
  );
};
