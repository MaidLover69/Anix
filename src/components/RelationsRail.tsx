import React, { useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { ShimmerImage } from './ShimmerImage';
import { Button } from './ui/Button';
import { useOpenExternal } from '../hooks/useOpenExternal';
import { mediaFormatLabel } from './library/format';
import type { RelationEdge } from '../types';
import { t, type TKey } from '../i18n';

interface RelationsRailProps {
  relations?: RelationEdge[];
}

const PRINT_FORMATS = ['MANGA', 'NOVEL', 'ONE_SHOT', 'COMIC', 'DOUJINSHI', 'MANHWA', 'MANHUA'];

function sentenceCase(value: string | null | undefined): string {
  if (!value) return '';
  const lower = value.replace(/_/g, ' ').toLowerCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

const RELATION_KEYS: Record<string, TKey> = {
  ADAPTATION: 'relationsRail.adaptation',
  PREQUEL: 'relationsRail.prequel',
  SEQUEL: 'relationsRail.sequel',
  PARENT: 'relationsRail.parent',
  SIDE_STORY: 'relationsRail.sideStory',
  CHARACTER: 'relationsRail.character',
  SUMMARY: 'relationsRail.summary',
  ALTERNATIVE: 'relationsRail.alternative',
  SPIN_OFF: 'relationsRail.spinOff',
  OTHER: 'relationsRail.other',
  SOURCE: 'relationsRail.source',
  COMPILATION: 'relationsRail.compilation',
  CONTAINS: 'relationsRail.contains',
};

function relationLabel(type: string | null | undefined): string {
  const key = type ? RELATION_KEYS[type] : undefined;
  return key ? t(key) : sentenceCase(type);
}

export const RelationsRail: React.FC<RelationsRailProps> = ({ relations = [] }) => {
  const navigate = useNavigate();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const openExternal = useOpenExternal();

  if (!relations || relations.length === 0) return null;

  const scroll = (direction: 'left' | 'right') => {
    scrollRef.current?.scrollBy({
      left: direction === 'left' ? -528 : 528,
      // An explicit behaviour overrides the CSS reduced-motion rule, so check it here.
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
    });
  };

  return (
    <section className="group/rail flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4">
        <h2 className="text-xl font-semibold text-text-primary">{t('relationsRail.related')}</h2>
        <div dir="ltr" className="flex items-center gap-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover/rail:opacity-100">
          <Button icon variant="ghost" size="sm" aria-label={t('relationsRail.scrollLeft')} onClick={() => scroll('left')}>
            <ChevronLeft aria-hidden="true" className="h-4 w-4" />
          </Button>
          <Button icon variant="ghost" size="sm" aria-label={t('relationsRail.scrollRight')} onClick={() => scroll('right')}>
            <ChevronRight aria-hidden="true" className="h-4 w-4" />
          </Button>
        </div>
      </div>

      <div
        ref={scrollRef}
        className="-mx-1 flex snap-x gap-4 overflow-x-auto px-1 py-1 scroll-px-1 no-scrollbar"
      >
        {relations.map((edge, index) => {
          const item = edge.node;
          if (!item) return null;

          const title =
            item.title?.english || item.title?.userPreferred || item.title?.romaji || t('relationsRail.untitled');
          const cover = item.coverImage?.extraLarge || item.coverImage?.large || null;

          const formatUpper = item.format?.toUpperCase() || '';
          const isPrint = PRINT_FORMATS.some((f) => formatUpper.includes(f));

          const handleClick = () => {
            if (isPrint) {
              // Manga and novels have no page in the app; open them in the browser.
              openExternal(
                item.idMal
                  ? `https://myanimelist.net/manga/${item.idMal}`
                  : `https://anilist.co/manga/${item.id}`
              );
            } else {
              navigate(`/detail/${item.id}`);
            }
          };

          const caption = [
            relationLabel(edge.relationType) || t('relationsRail.related'),
            mediaFormatLabel(item.format),
            item.averageScore ? `${item.averageScore}%` : null,
          ]
            .filter(Boolean)
            .join(' · ');

          return (
            <button
              key={`${item.id ?? index}-${edge.relationType ?? ''}`}
              type="button"
              onClick={handleClick}
              title={isPrint ? t('relationsRail.openInBrowser', { title }) : title}
              className="group flex w-40 flex-shrink-0 cursor-pointer snap-start flex-col rounded-xl text-start"
            >
              <div className="relative aspect-[2/3] w-full overflow-hidden rounded-xl ui-card bg-bg-card">
                <ShimmerImage
                  src={cover}
                  alt=""
                  className="h-full w-full transition-transform duration-150 group-hover:scale-[1.04]"
                />
              </div>
              <span className="mt-2 text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
                {title}
              </span>
              <span className="mt-0.5 w-full truncate text-xs text-text-muted">{caption}</span>
            </button>
          );
        })}
      </div>
    </section>
  );
};
