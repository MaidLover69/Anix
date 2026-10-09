import React from 'react';
import { Bookmark } from 'lucide-react';
import type { UseQueryResult } from '@tanstack/react-query';
import { EmptyState, ErrorState } from '../ui';
import { AnimeCard } from '../AnimeCard';
import { useProgressiveList } from '../../hooks/useProgressiveList';
import type { WatchlistEntry } from '../../types';
import { mediaFromListRow } from './format';
import { PosterGrid, PosterGridSkeleton, ShowMore } from './PosterGrid';

interface WatchlistTabProps {
  query: UseQueryResult<WatchlistEntry[]>;
}

export const WatchlistTab: React.FC<WatchlistTabProps> = ({ query }) => {
  const watchlist = query.data ?? [];
  const list = useProgressiveList(watchlist);

  if (query.isLoading) return <PosterGridSkeleton />;

  if (query.isError) {
    return (
      <ErrorState
        title="Could not load your watchlist"
        error={query.error}
        onRetry={() => query.refetch()}
      />
    );
  }

  if (watchlist.length === 0) {
    return (
      <EmptyState
        icon={<Bookmark className="w-8 h-8" aria-hidden="true" />}
        title="Your watchlist is empty"
        description="Use the bookmark on any anime in Home or Discover to save it here."
      />
    );
  }

  return (
    <>
      <PosterGrid>
        {list.visible.map((item) => (
          <AnimeCard
            key={item.anilist_id}
            media={mediaFromListRow({
              id: item.anilist_id,
              title: item.title,
              cover: item.cover,
              format: item.format,
              averageScore: item.average_score,
            })}
          />
        ))}
      </PosterGrid>
      {list.hasMore && (
        <ShowMore
          sentinelRef={list.sentinelRef}
          onClick={list.showMore}
          remaining={watchlist.length - list.visible.length}
        />
      )}
    </>
  );
};
