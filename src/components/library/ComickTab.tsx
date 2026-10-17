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

/** Comick's status numbers, in the order of the AniList tab. */
const COMICK_STATUSES: { value: string; label: string }[] = [
  { value: '1', label: 'Watching' },
  { value: '5', label: 'Planning' },
  { value: '2', label: 'Completed' },
  { value: '3', label: 'On hold' },
  { value: '4', label: 'Dropped' },
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
        title="Comick is not connected"
        description="Connect your Comick account to see its anime library here and have the episodes you finish saved to it."
        action={
          <Button variant="primary" onClick={() => setAuthDialogOpen(true)}>
            Connect Comick
          </Button>
        }
      />
    );
  }

  const statusLabel = COMICK_STATUSES.find((option) => option.value === status)?.label ?? 'this list';

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
        <SegmentedControl aria-label="Comick list" options={COMICK_STATUSES} value={status} onChange={setStatus} />
        {accountsQuery.data?.comick.name && (
          <span className="truncate text-xs font-medium text-text-secondary">{accountsQuery.data.comick.name}</span>
        )}
      </div>

      {libraryQuery.isLoading ? (
        <PosterGridSkeleton />
      ) : libraryQuery.isError ? (
        <ErrorState
          title="Could not load your Comick library"
          message={errorMessage(libraryQuery.error)}
          onRetry={() => libraryQuery.refetch()}
        />
      ) : entries.length === 0 ? (
        <EmptyState
          icon={<ListChecks className="w-8 h-8" aria-hidden="true" />}
          title={`Nothing in ${statusLabel}`}
          description="Anime on this list of your Comick account shows up here."
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
              title={entry.url ? `Open ${entry.title} on Comick` : undefined}
              className="group flex flex-col gap-2 text-left focus-visible:outline-none"
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
            <span className="mt-1 truncate pl-0.5 text-[11px] tabular-nums text-text-muted">
              Progress {entry.progress?.number ?? 0}/{entry.total_units ?? '?'}
              {entry.rating !== null && ` · Score ${entry.rating}`}
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
