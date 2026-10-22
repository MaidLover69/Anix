import React from 'react';
import { Button } from '../ui';
import { SkeletonCard } from '../SkeletonCard';
import { t } from '../../i18n';

/** Poster columns sized by the cards, so they do not balloon on wide windows. */
export const PosterGrid: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-6">{children}</div>
);

export const PosterGridSkeleton: React.FC<{ count?: number }> = ({ count = 12 }) => (
  <PosterGrid>
    {Array.from({ length: count }, (_, index) => (
      <SkeletonCard key={index} />
    ))}
  </PosterGrid>
);

interface ShowMoreProps {
  sentinelRef: (node: HTMLElement | null) => void;
  onClick: () => void;
  remaining: number;
}

/** End-of-list marker: the next step loads when it scrolls into range, or on click. */
export const ShowMore: React.FC<ShowMoreProps> = ({ sentinelRef, onClick, remaining }) => (
  <div ref={sentinelRef} className="flex justify-center pt-6">
    <Button variant="ghost" size="sm" onClick={onClick}>
      {t('posterGrid.showMore', { count: remaining })}
    </Button>
  </div>
);
