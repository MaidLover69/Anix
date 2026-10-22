import React, { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Cloud, ExternalLink, ListChecks } from 'lucide-react';
import { Button, EmptyState, ErrorState, SegmentedControl, Spinner, errorMessage } from '../ui';
import { useAccountStatus } from '../../hooks/useAccounts';
import { useOpenExternal } from '../../hooks/useOpenExternal';
import { useProgressiveList } from '../../hooks/useProgressiveList';
import { useUIStore } from '../../stores/uiStore';
import type { ComickLibraryEntry } from '../../types';
import { CoverThumb } from './CoverThumb';
import { PosterGrid, PosterGridSkeleton, ShowMore } from './PosterGrid';
import { t } from '../../i18n';

/** Comick's status numbers, in the order of the AniList tab. Labels are getters so they follow the interface language. */
const COMICK_STATUSES: { value: string; label: string }[] = [
  { value: '1', get label() { return t('comickTab.watching'); } },
  { value: '5', get label() { return t('comickTab.planning'); } },
  { value: '2', get label() { return t('comickTab.completed'); } },
  { value: '3', get label() { return t('comickTab.onHold'); } },
  { value: '4', get label() { return t('comickTab.dropped'); } },
];

function useComickLibrary(enabled: boolean) {
  return useQuery<ComickLibraryEntry[]>({
    queryKey: ['comickLibrary'],
    queryFn: () => invoke<ComickLibraryEntry[]>('get_comick_library'),
    enabled,
    staleTime: 5 * 60 * 1000,
  });
}

/**
 * The connected Comick account's anime library. Comick titles carry no AniList id here, so
 * a card opens the title on Comick.
 */
export const ComickTab: React.FC = () => {
  const setAuthDialogOpen = useUIStore((state) => state.setAuthDialogOpen);
  const accountsQuery = useAccountStatus();
  const connected = accountsQuery.data?.comick.connected ?? false;
  const libraryQuery = useComickLibrary(connected);
  const [status, setStatus] = useState(COMICK_STATUSES[0].value);

  const entries = useMemo(
    () => (libraryQuery.data ?? []).filter((entry) => String(entry.status) === status),
    [libraryQuery.data, status],
  );

  if (accountsQuery.isLoading) {
    return (
      <div className="flex justify-center py-20">
        <Spinner size="lg" className="text-text-muted" />
      </div>
    );
  }

  if (!connected) {
    return (
      <EmptyState
        icon={<Cloud className="w-8 h-8" aria-hidden="true" />}
        title={t('comickTab.notConnected')}
        description={t('comickTab.notConnectedHint')}
        action={
          <Button variant="primary" onClick={() => setAuthDialogOpen(true)}>
            {t('comickTab.connect')}
          </Button>
        }
      />
    );
  }

  const statusLabel = COMICK_STATUSES.find((option) => option.value === status)?.label ?? t('comickTab.thisList');

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
        <SegmentedControl aria-label={t('comickTab.listLabel')} options={COMICK_STATUSES} value={status} onChange={setStatus} />
        {accountsQuery.data?.comick.name && (
          <span className="truncate text-xs font-medium text-text-secondary">{accountsQuery.data.comick.name}</span>
        )}
      </div>

      {libraryQuery.isLoading ? (
        <PosterGridSkeleton />
      ) : libraryQuery.isError ? (
        <ErrorState
          title={t('comickTab.libraryError')}
          message={errorMessage(libraryQuery.error)}
          onRetry={() => libraryQuery.refetch()}
        />
      ) : entries.length === 0 ? (
        <EmptyState
          icon={<ListChecks className="w-8 h-8" aria-hidden="true" />}
          title={t('comickTab.emptyTitle', { status: statusLabel })}
          description={t('comickTab.emptyHint')}
        />
      ) : (
        <ComickEntries key={status} entries={entries} />
      )}
    </div>
  );
};

const ComickEntries: React.FC<{ entries: ComickLibraryEntry[] }> = ({ entries }) => {
  const list = useProgressiveList(entries);
  const openExternal = useOpenExternal();

  return (
    <>
      <PosterGrid>
        {list.visible.map((entry) => (
          <div key={entry.hid} className="flex min-w-0 flex-col">
            <button
              type="button"
              onClick={() => entry.url && openExternal(entry.url)}
              disabled={!entry.url}
              title={entry.url ? t('comickTab.openOnComick', { title: entry.title }) : undefined}
              className="group flex flex-col gap-2 text-start focus-visible:outline-none"
            >
              <CoverThumb
                src={entry.cover_url}
                className="aspect-[2/3] w-full rounded-lg object-cover ring-accent-500/60 group-focus-visible:ring-2"
              />
              <span className="flex items-start gap-1 text-[13px] font-medium leading-snug text-text-primary">
                <span className="line-clamp-2">{entry.title}</span>
                {entry.url && (
                  <ExternalLink aria-hidden="true" className="mt-0.5 h-3 w-3 flex-shrink-0 text-text-muted" />
                )}
              </span>
            </button>
            <span className="mt-1 truncate ps-0.5 text-[11px] tabular-nums text-text-muted">
              {t('comickTab.progress', { progress: entry.progress?.number ?? 0, total: entry.total_units ?? '?' })}
              {entry.rating !== null && ` · ${t('comickTab.score', { score: entry.rating })}`}
            </span>
          </div>
        ))}
      </PosterGrid>
      {list.hasMore && (
        <ShowMore
          sentinelRef={list.sentinelRef}
          onClick={list.showMore}
          remaining={entries.length - list.visible.length}
        />
      )}
    </>
  );
};
