import React from 'react';
import { Link } from 'react-router-dom';
import { History } from 'lucide-react';
import type { UseQueryResult } from '@tanstack/react-query';
import { EmptyState, ErrorState, Spinner } from '../ui';
import type { HistoryEntry } from '../../types';
import { watchPath } from '../../hooks/watchProgress';
import { categoryLabel, episodeLabel, watchedPercent } from './format';
import { CoverThumb } from './CoverThumb';

interface HistoryTabProps {
  query: UseQueryResult<HistoryEntry[]>;
}

export const HistoryTab: React.FC<HistoryTabProps> = ({ query }) => {
  if (query.isLoading) {
    return (
      <div className="flex justify-center py-20">
        <Spinner size="lg" label="Loading watch history" className="text-text-muted" />
      </div>
    );
  }

  if (query.isError) {
    return (
      <ErrorState
        title="Could not load watch history"
        error={query.error}
        onRetry={() => query.refetch()}
      />
    );
  }

  const history = query.data ?? [];

  if (history.length === 0) {
    return (
      <EmptyState
        icon={<History className="w-8 h-8" aria-hidden="true" />}
        title="No watch history yet"
        description="The last episode you watched of each anime shows up here so you can pick it up again."
      />
    );
  }

  return (
    <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
      {history.map((entry) => {
        const percent = watchedPercent(entry.position_ms, entry.duration_ms);
        const episode = `${episodeLabel(entry.episode_number)} · ${categoryLabel(entry.category)}`;

        return (
          <li
            key={`${entry.anilist_id}-${entry.episode_number}`}
            className="[content-visibility:auto] [contain-intrinsic-size:auto_190px]"
          >
            <Link
              to={watchPath(entry.anilist_id, entry.episode_number, entry.category, entry.provider?.trim())}
              aria-label={`Continue ${entry.title}, ${episode}`}
              className="flex flex-col gap-2 p-2 rounded-lg bg-bg-card hover:bg-bg-elevated transition-colors focus-visible:-outline-offset-2"
            >
              <div className="relative w-full aspect-video rounded-md overflow-hidden bg-bg-elevated">
                <CoverThumb src={entry.cover} className="w-full h-full" />
                {percent !== null && (
                  <div className="absolute bottom-0 left-0 right-0 h-1 bg-white/20">
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
                    <span className="flex-shrink-0 tabular-nums">{percent}% watched</span>
                  )}
                </span>
              </div>
            </Link>
          </li>
        );
      })}
    </ul>
  );
};
