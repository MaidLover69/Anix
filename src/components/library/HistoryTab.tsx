import React from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Compass, History } from 'lucide-react';
import type { UseQueryResult } from '@tanstack/react-query';
import { Button, EmptyState, ErrorState, Spinner } from '../ui';
import type { HistoryEntry } from '../../types';
import { isHistoryEntryFinished, watchPath } from '../../hooks/watchProgress';
import { usePlayful } from '../../hooks/useSettings';
import { categoryLabel, episodeLabel, watchedPercent } from './format';
import { CoverThumb } from './CoverThumb';
import { ShowMore } from './PosterGrid';
import { useProgressiveList } from '../../hooks/useProgressiveList';
import { t } from '../../i18n';

interface HistoryTabProps {
  query: UseQueryResult<HistoryEntry[]>;
}

export const HistoryTab: React.FC<HistoryTabProps> = ({ query }) => {
  const playful = usePlayful();
  const navigate = useNavigate();
  const history = query.data ?? [];
  // The whole history can be long; render it in steps like the other Library lists.
  const list = useProgressiveList(history);
  if (query.isLoading) {
    return (
      <div className="flex justify-center py-20">
        <Spinner size="lg" label={t('historyTab.loading')} className="text-text-muted" />
      </div>
    );
  }

  if (query.isError) {
    return (
      <ErrorState
        title={t('historyTab.loadError')}
        error={query.error}
        onRetry={() => query.refetch()}
      />
    );
  }

  if (history.length === 0) {
    return (
      <EmptyState
        icon={<History className="w-8 h-8" aria-hidden="true" />}
        title={playful ? t('historyTab.emptyPlayful') : t('historyTab.empty')}
        description={
          playful
            ? t('historyTab.emptyHintPlayful')
            : t('historyTab.emptyHint')
        }
        mood="sleepy"
        kaomoji="(-.-)"
        action={
          <Button variant="primary" onClick={() => navigate('/discover')}>
            <Compass className="h-4 w-4" aria-hidden="true" />
            {t('historyTab.browseAnime')}
          </Button>
        }
      />
    );
  }

  return (
    <>
      <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
        {list.visible.map((entry) => {
          const percent = watchedPercent(entry.position_ms, entry.duration_ms);
          const episode = `${episodeLabel(entry.episode_number)} · ${categoryLabel(entry.category)}`;
          // A finished episode goes to the anime page, which knows the next released episode.
          const finished = isHistoryEntryFinished(entry);

          return (
            <li
              key={`${entry.anilist_id}-${entry.episode_number}`}
              className="[content-visibility:auto] [contain-intrinsic-size:auto_190px]"
            >
              <Link
                to={
                  finished
                    ? `/detail/${entry.anilist_id}`
                    : watchPath(entry.anilist_id, entry.episode_number, entry.category, entry.provider?.trim())
                }
                aria-label={
                  finished
                    ? t('historyTab.finishedLabel', { title: entry.title, episode })
                    : t('historyTab.continueLabel', { title: entry.title, episode })
                }
                className="flex flex-col gap-2 p-2 rounded-lg ui-card bg-bg-card hover:bg-bg-elevated transition-colors focus-visible:-outline-offset-2"
              >
                <div className="relative w-full aspect-video rounded-md overflow-hidden bg-bg-elevated">
                  <CoverThumb src={entry.cover} className="w-full h-full" />
                  {percent !== null && (
                    <div className="absolute bottom-0 start-0 end-0 h-1 bg-white/20">
                      <div className="h-full bg-accent-500" style={{ width: `${percent}%` }} />
                    </div>
                  )}
                </div>

                <div className="flex flex-col gap-0.5 px-1 pb-1 min-w-0">
                  <span className="text-[13px] font-medium text-text-primary truncate" title={entry.title}>
                    {entry.title}
                  </span>
                  <span className="flex items-center justify-between gap-2 text-xs text-text-muted">
                    <span className="truncate">{episode}</span>
                    {percent !== null && (
                      <span className="flex-shrink-0 tabular-nums">{t('historyTab.watchedPercent', { percent })}</span>
                    )}
                  </span>
                </div>
              </Link>
            </li>
          );
        })}
      </ul>
      {list.hasMore && (
        <ShowMore
          sentinelRef={list.sentinelRef}
          onClick={list.showMore}
          remaining={history.length - list.visible.length}
        />
      )}
    </>
  );
};
