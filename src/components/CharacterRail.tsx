import React, { useRef } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { ShimmerImage } from './ShimmerImage';
import { Button } from './ui/Button';
import { useOpenExternal } from '../hooks/useOpenExternal';
import type { CharacterEdge } from '../types';

interface CharacterRailProps {
  characters?: CharacterEdge[];
}

function sentenceCase(value: string): string {
  const lower = value.replace(/_/g, ' ').toLowerCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

export const CharacterRail: React.FC<CharacterRailProps> = ({ characters = [] }) => {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const openExternal = useOpenExternal();

  if (!characters || characters.length === 0) return null;

  const scroll = (direction: 'left' | 'right') => {
    scrollRef.current?.scrollBy({ left: direction === 'left' ? -672 : 672, behavior: 'smooth' });
  };

  return (
    <section className="group/rail flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4">
        <h2 className="text-xl font-semibold text-text-primary">Characters</h2>
        <div className="flex items-center gap-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover/rail:opacity-100">
          <Button icon variant="ghost" size="sm" aria-label="Scroll characters left" onClick={() => scroll('left')}>
            <ChevronLeft aria-hidden="true" className="h-4 w-4" />
          </Button>
          <Button icon variant="ghost" size="sm" aria-label="Scroll characters right" onClick={() => scroll('right')}>
            <ChevronRight aria-hidden="true" className="h-4 w-4" />
          </Button>
        </div>
      </div>

      <div
        ref={scrollRef}
        className="-mx-1 flex snap-x gap-4 overflow-x-auto px-1 py-1 scroll-px-1 no-scrollbar"
      >
        {characters.map((edge, index) => {
          const char = edge.node;
          const va = edge.voiceActors?.[0];
          if (!char) return null;

          const charName = char.name?.full || 'Unknown';
          const vaName = va?.name?.full || 'Voice actor';

          return (
            <div
              key={char.id ?? `index-${index}`}
              className="flex w-80 flex-shrink-0 snap-start items-center justify-between gap-3 rounded-xl ui-card bg-bg-card p-2"
            >
              <div className="flex min-w-0 flex-1 items-center gap-3">
                <ShimmerImage
                  src={char.image?.large || char.image?.medium}
                  alt=""
                  className="h-16 w-12 flex-shrink-0 rounded-md"
                />
                <div className="flex min-w-0 flex-col">
                  <span
                    title={charName}
                    className="text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2 select-text"
                  >
                    {charName}
                  </span>
                  <span className="truncate text-xs text-text-muted">
                    {sentenceCase(edge.role || 'Supporting')}
                  </span>
                </div>
              </div>

              {va && (
                <button
                  type="button"
                  onClick={() => openExternal(`https://anilist.co/staff/${va.id}`)}
                  title={`Open ${vaName} on AniList`}
                  className="group/va flex min-w-0 flex-1 cursor-pointer items-center justify-end gap-3 rounded-md text-right"
                >
                  <div className="flex min-w-0 flex-col">
                    <span className="text-[13px] font-medium leading-[18px] text-text-secondary transition-colors line-clamp-2 group-hover/va:text-accent-300">
                      {vaName}
                    </span>
                    <span className="truncate text-xs text-text-muted">
                      {va.languageV2 || 'Japanese'}
                    </span>
                  </div>
                  <ShimmerImage
                    src={va.image?.large || va.image?.medium}
                    alt=""
                    className="h-16 w-12 flex-shrink-0 rounded-md"
                  />
                </button>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
};
