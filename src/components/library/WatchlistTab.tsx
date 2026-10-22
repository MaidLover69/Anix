import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Bookmark, ListFilter } from 'lucide-react';
import type { UseQueryResult } from '@tanstack/react-query';
import { Button, Chip, EmptyState, ErrorState } from '../ui';
import { AnimeCard } from '../AnimeCard';
import { useProgressiveList } from '../../hooks/useProgressiveList';
import { usePlayful } from '../../hooks/useSettings';
import { useAccountStatus } from '../../hooks/useAccounts';
import { LIST_STATUS_OPTIONS, useSyncListStatuses } from '../../hooks/useLibrary';
import type { ListStatusValue, WatchlistEntry } from '../../types';
import { mediaFromListRow } from './format';
import { PosterGrid, PosterGridSkeleton, ShowMore } from './PosterGrid';
import { t } from '../../i18n';

interface WatchlistTabProps {
  query: UseQueryResult<WatchlistEntry[]>;
}

export const WatchlistTab: React.FC<WatchlistTabProps> = ({ query }) => {
  const watchlist = query.data ?? [];
  const [filter, setFilter] = useState<ListStatusValue | null>(null);
  const shown = filter === null ? watchlist : watchlist.filter((item) => item.status === filter);
  const list = useProgressiveList(shown, 60, filter);
  const playful = usePlayful();
  const navigate = useNavigate();
  const accounts = useAccountStatus().data;
  // With AniList connected its statuses win; read them in once per visit.
  useSyncListStatuses(Boolean(accounts?.anilist.connected));

  if (query.isLoading) return <PosterGridSkeleton />;

  if (query.isError) {
    return (
      <ErrorState
        title={t('watchlistTab.loadError')}
        error={query.error}
        onRetry={() => query.refetch()}
      />
    );
  }

  if (watchlist.length === 0) {
    return (
      <EmptyState
        icon={<Bookmark className="w-8 h-8" aria-hidden="true" />}
        title={playful ? t('watchlistTab.emptyPlayful') : t('watchlistTab.empty')}
        description={
          playful
            ? t('watchlistTab.emptyHintPlayful')
            : t('watchlistTab.emptyHint')
        }
        mood="sleepy"
        kaomoji="(._.)"
        action={
          <Button variant="primary" onClick={() => navigate('/discover')}>
            {t('watchlistTab.browseAnime')}
          </Button>
        }
      />
    );
  }

  const countOf = (status: ListStatusValue) => watchlist.filter((item) => item.status === status).length;
  const chips: { value: ListStatusValue | null; label: string; count: number }[] = [
    { value: null, label: t('watchlistTab.all'), count: watchlist.length },
    ...LIST_STATUS_OPTIONS.map((option) => ({ ...option, count: countOf(option.value) })),
  ];
  const filterLabel = chips.find((chip) => chip.value === filter)?.label ?? t('watchlistTab.all');

  return (
    <>
      <div role="group" aria-label={t('watchlistTab.filterLabel')} className="flex flex-wrap gap-2">
        {chips.map((chip) => (
          <Chip key={chip.label} selected={filter === chip.value} onClick={() => setFilter(chip.value)}>
            {chip.label}
            <span className="ms-1.5 tabular-nums opacity-70">{chip.count}</span>
          </Chip>
        ))}
      </div>

      {shown.length === 0 ? (
        <EmptyState
          icon={<ListFilter className="w-8 h-8" aria-hidden="true" />}
          title={t('watchlistTab.filterEmpty', { status: filterLabel })}
          description={t('watchlistTab.filterEmptyHint')}
        />
      ) : (
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
      )}
      {list.hasMore && (
        <ShowMore
          sentinelRef={list.sentinelRef}
          onClick={list.showMore}
          remaining={shown.length - list.visible.length}
        />
      )}
    </>
  );
};
