import React from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Compass, History, Play } from 'lucide-react';
import type { UseQueryResult } from '@tanstack/react-query';
import { Button, EmptyState, ErrorState, Spinner } from '../ui';
import type { HistoryEntry } from '../../types';
import { isHistoryEntryFinished, watchPath } from '../../hooks/watchProgress';
import { useMemeLevel, usePlayful } from '../../hooks/useSettings';
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
  const high = useMemeLevel() === 'high';
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
        title={playful && !high ? t('historyTab.emptyPlayful') : t('historyTab.empty')}
        meme="histEmpty"
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
      <ul className="flex flex-col gap-1">
        {list.visible.map((entry) => {
          const percent = watchedPercent(entry.position_ms, entry.duration_ms);
          const episode = `${episodeLabel(entry.episode_number)} · ${categoryLabel(entry.category)}`;
          // A finished episode goes to the anime page, which knows the next released episode.
          const finished = isHistoryEntryFinished(entry);

          return (
            <li
              key={`${entry.anilist_id}-${entry.episode_number}`}
              className="[content-visibility:auto] [contain-intrinsic-size:auto_80px]"
            >
              {/* One link per row; the Continue pill is only its visible cue. */}
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
                className="group flex items-center gap-3 rounded-lg p-2 transition-colors hover:bg-ink/5 focus-visible:-outline-offset-2"
              >
                <CoverThumb src={entry.cover} className="aspect-[2/3] w-12 flex-shrink-0 rounded-md" />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="truncate text-[13px] font-medium text-text-primary" title={entry.title}>
                    {entry.title}
                  </span>
                  <span className="truncate text-xs text-text-muted">
                    {percent !== null ? `${episode} · ${t('historyTab.watchedPercent', { percent })}` : episode}
                  </span>
                  {percent !== null && (
                    <span aria-hidden="true" className="mt-1 h-1 w-full max-w-72 overflow-hidden rounded-full bg-ink/10">
                      <span className="block h-full rounded-full bg-accent-500" style={{ width: `${percent}%` }} />
                    </span>
                  )}
                </span>
                <span
                  aria-hidden="true"
                  className="inline-flex h-8 flex-shrink-0 items-center gap-1.5 rounded-md bg-accent-600 px-3 text-[13px] font-medium text-on-accent transition-colors group-hover:bg-accent-700"
                >
                  <Play className="h-3.5 w-3.5 fill-current" />
                  {t('historyTab.continue')}
                </span>
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
