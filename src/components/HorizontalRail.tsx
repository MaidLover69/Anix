import React, { useRef } from 'react';
import { scrollBehavior } from '../lib/motion';
import { Link } from 'react-router-dom';
import { ArrowRight, ChevronLeft, ChevronRight } from 'lucide-react';
import { AnimeCard } from './AnimeCard';
import { SkeletonCard } from './SkeletonCard';
import { Button } from './ui/Button';
import { ContinueCard } from './ContinueCard';
import { t } from '../i18n';
import { Meme, MemeFrame, type MemeSpot } from './meme/Meme';
import type { Media, HistoryEntry } from '../types';

interface HorizontalRailProps {
  title: string;
  /** Meme mode: the title's meme, and its face after the title when the spot has an image (High). */
  titleSpot?: MemeSpot;
  items?: Media[];
  historyItems?: HistoryEntry[];
  isLoading?: boolean;
  viewAllLink?: string;
}

const CARD_CLASS = 'w-44 flex-shrink-0 snap-start';

export const HorizontalRail: React.FC<HorizontalRailProps> = ({
  title,
  titleSpot,
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
      behavior: scrollBehavior(),
    });
  };

  const hasContent = (items && items.length > 0) || (historyItems && historyItems.length > 0);
  if (!isLoading && !hasContent) return null;

  return (
    <section className="group/rail flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4">
        <h2 className="ui-rail-title text-xl font-semibold text-text-primary">
          {titleSpot ? <Meme spot={titleSpot} plain={title} /> : title}
          {titleSpot && <MemeFrame spot={titleSpot} className="meme-face" />}
        </h2>

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
              className="ui-view-all -me-3 flex h-8 items-center gap-1.5 rounded-md px-3 text-[13px] font-medium text-text-secondary transition-colors hover:bg-ink/10 hover:text-text-primary"
            >
              {t('horizontalRail.viewAll')}
              <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
            </Link>
          )}
        </div>
      </div>

      <div
        ref={scrollRef}
        // Room above and below for a card that lifts and tilts on hover; the scroller clips its overflow.
        className="-mx-1 -my-1 flex snap-x snap-mandatory scroll-px-1 items-start gap-4 overflow-x-auto px-1 py-2 no-scrollbar"
      >
        {isLoading && !hasContent
          ? Array.from({ length: 7 }).map((_, i) => <SkeletonCard key={i} className="w-44" />)
          : historyItems && historyItems.length > 0
          ? historyItems.map((hist) => (
              <ContinueCard key={hist.anilist_id} entry={hist} className="w-[clamp(15rem,22vw,19rem)]" />
            ))
          : items?.map((anime) => <AnimeCard key={anime.id} media={anime} className={CARD_CLASS} />)}
      </div>
    </section>
  );
};
