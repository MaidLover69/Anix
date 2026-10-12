import React, { useState } from 'react';
import { Cloud, ListChecks, Pencil } from 'lucide-react';
import { Button, EmptyState, ErrorState, SegmentedControl, Spinner, errorMessage } from '../ui';
import { AnimeCard } from '../AnimeCard';
import { useAniListLibrary, useAniListViewer } from '../../hooks/useAniList';
import { useAccountStatus } from '../../hooks/useAccounts';
import { useProgressiveList } from '../../hooks/useProgressiveList';
import { useUIStore } from '../../stores/uiStore';
import type { MediaListEntry, Viewer } from '../../types';
import { EditListEntryModal } from './EditListEntryModal';
import { ANILIST_LIST_STATUSES, mediaFromListRow } from './format';
import { PosterGrid, PosterGridSkeleton, ShowMore } from './PosterGrid';

/**
 * The connected AniList account's lists. Episodes finished in Anix are sent to AniList by
 * the backend, which announces each change so the list here is refetched; an entry's status
 * and progress can also be edited by hand.
 */
export const AniListTab: React.FC = () => {
  const setAuthDialogOpen = useUIStore((state) => state.setAuthDialogOpen);
  const accountsQuery = useAccountStatus();
  const connected = accountsQuery.data?.anilist.connected ?? false;
  const viewerQuery = useAniListViewer(connected);
  const viewer = viewerQuery.data ?? null;

  const [status, setStatus] = useState<string>(ANILIST_LIST_STATUSES[0].value);
  const entriesQuery = useAniListLibrary(connected && viewer !== null, status);

  if (accountsQuery.isLoading) {
    return (
      <div className="flex justify-center py-20">
        <Spinner size="lg" className="text-text-muted" />
      </div>
    );
  }

  if (accountsQuery.isError) {
    return (
      <ErrorState
        title="Could not load your accounts"
        error={accountsQuery.error}
        onRetry={() => accountsQuery.refetch()}
      />
    );
  }

  if (!connected) {
    return (
      <EmptyState
        icon={<Cloud className="w-8 h-8" aria-hidden="true" />}
        title="AniList is not connected"
        description="Connect your AniList account to see your lists here and have the episodes you finish saved to them."
        action={
          <Button variant="primary" onClick={() => setAuthDialogOpen(true)}>
            Connect AniList
          </Button>
        }
      />
    );
  }

  if (viewerQuery.isLoading) {
    return (
      <div className="flex flex-col items-center gap-3 py-20 text-xs text-text-muted">
        <Spinner size="lg" label="Loading your AniList account" />
        <span aria-hidden="true">Loading your AniList account…</span>
      </div>
    );
  }

  if (viewerQuery.isError || !viewer) {
    return (
      <ErrorState
        title="Could not load your AniList account"
        message={
          viewerQuery.isError
            ? errorMessage(viewerQuery.error)
            : 'AniList did not return an account for the saved login.'
        }
        onRetry={() => viewerQuery.refetch()}
        action={
          <Button variant="ghost" onClick={() => setAuthDialogOpen(true)}>
            Accounts
          </Button>
        }
      />
    );
  }

  const statusLabel =
    ANILIST_LIST_STATUSES.find((option) => option.value === status)?.label ?? 'this list';

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
        <SegmentedControl
          aria-label="AniList list"
          options={ANILIST_LIST_STATUSES}
          value={status}
          onChange={setStatus}
        />
        <ViewerBadge viewer={viewer} />
      </div>

      <AniListEntries
        key={status}
        entries={entriesQuery.data ?? []}
        isLoading={entriesQuery.isLoading}
        error={entriesQuery.isError ? errorMessage(entriesQuery.error) : null}
        onRetry={() => entriesQuery.refetch()}
        statusLabel={statusLabel}
      />
    </div>
  );
};

const ViewerBadge: React.FC<{ viewer: Viewer }> = ({ viewer }) => {
  const [avatarFailed, setAvatarFailed] = useState(false);

  return (
    <div className="flex items-center gap-2 min-w-0 text-xs text-text-muted">
      {viewer.avatar_url && !avatarFailed && (
        <img
          src={viewer.avatar_url}
          alt=""
          onError={() => setAvatarFailed(true)}
          className="w-6 h-6 rounded-full object-cover flex-shrink-0"
        />
      )}
      <span className="truncate text-text-secondary font-medium">{viewer.name}</span>
    </div>
  );
};

interface AniListEntriesProps {
  entries: MediaListEntry[];
  isLoading: boolean;
  error: string | null;
  onRetry: () => void;
  statusLabel: string;
}

const AniListEntries: React.FC<AniListEntriesProps> = ({
  entries,
  isLoading,
  error,
  onRetry,
  statusLabel,
}) => {
  const list = useProgressiveList(entries);
  const [editing, setEditing] = useState<MediaListEntry | null>(null);

  if (isLoading) return <PosterGridSkeleton />;

  if (error !== null) {
    return <ErrorState title="Could not load this AniList list" message={error} onRetry={onRetry} />;
  }

  if (entries.length === 0) {
    return (
      <EmptyState
        icon={<ListChecks className="w-8 h-8" aria-hidden="true" />}
        title={`Nothing in ${statusLabel}`}
        description="Anime on this list of your AniList account shows up here."
      />
    );
  }

  return (
    <>
      <PosterGrid>
        {list.visible.map((entry) => (
          <div key={entry.id} className="flex flex-col min-w-0">
            <AnimeCard
              media={mediaFromListRow({
                id: entry.id,
                title: entry.title,
                cover: entry.cover,
                format: entry.format_str,
                averageScore: entry.average_score,
                episodes: entry.total_episodes,
              })}
            />
            <div className="flex items-center justify-between gap-2 pl-0.5 mt-1 text-[11px] text-text-muted tabular-nums">
              <span className="truncate">
                Progress {entry.progress}/{entry.total_episodes ?? '?'}
                {entry.score > 0 && ` · Score ${entry.score}`}
              </span>
              <Button
                icon
                variant="ghost"
                size="sm"
                aria-label={`Edit ${entry.title ?? 'this entry'} on your list`}
                onClick={() => setEditing(entry)}
                className="-my-1"
              >
                <Pencil aria-hidden="true" className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
        ))}
      </PosterGrid>
      {editing && <EditListEntryModal entry={editing} onClose={() => setEditing(null)} />}
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
