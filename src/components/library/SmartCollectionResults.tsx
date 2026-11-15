/**
 * Library › Playlists › a smart collection: the titles its saved Discover filters find today
 * (first page, same order and size as Discover), with a way to open the same search in Discover.
 */
import React from 'react';
import { useNavigate } from 'react-router-dom';
import { SearchX, SlidersHorizontal } from 'lucide-react';
import { AnimeCard } from '../AnimeCard';
import { SkeletonCard } from '../SkeletonCard';
import { Button, EmptyState, ErrorState } from '../ui';
import { useSearchAnime } from '../../hooks/useAniList';
import { discoverLink, searchFromRule } from '../../lib/smartRule';
import { t } from '../../i18n';

const GRID_CLASS = 'grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-6';
const PLACEHOLDER_CARDS = 12;

export const SmartCollectionResults: React.FC<{ rule: string }> = ({ rule }) => {
  const navigate = useNavigate();
  const search = useSearchAnime(searchFromRule(rule));
  const results = search.data ?? [];

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <p className="flex-1 min-w-0 text-[13px] text-text-secondary">{t('smartCollection.hint')}</p>
        <Button variant="secondary" onClick={() => navigate(discoverLink(rule))}>
          <SlidersHorizontal className="w-4 h-4" aria-hidden="true" />
          <span>{t('smartCollection.openInDiscover')}</span>
        </Button>
      </div>
      {search.isLoading ? (
        <div className={GRID_CLASS} aria-busy="true" aria-label={t('smartCollection.loading')}>
          {Array.from({ length: PLACEHOLDER_CARDS }, (_, i) => (
            <SkeletonCard key={i} />
          ))}
        </div>
      ) : search.isError ? (
        <ErrorState title={t('smartCollection.loadError')} error={search.error} onRetry={() => search.refetch()} />
      ) : results.length === 0 ? (
        <EmptyState
          icon={<SearchX className="w-8 h-8" aria-hidden="true" />}
          title={t('smartCollection.empty')}
          description={t('smartCollection.emptyHint')}
        />
      ) : (
        <div className={GRID_CLASS}>
          {results.map((anime) => (
            <AnimeCard key={anime.id} media={anime} />
          ))}
        </div>
      )}
    </div>
  );
};
