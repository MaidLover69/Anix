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
import { mediaFromListRow } from './format';
import { ANILIST_STATUS_OPTIONS } from '../../hooks/useLibrary';
import { PosterGrid, PosterGridSkeleton, ShowMore } from './PosterGrid';
import { t } from '../../i18n';

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

  const [status, setStatus] = useState<string>(ANILIST_STATUS_OPTIONS[0].value);
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
        title={t('aniListTab.accountsError')}
        error={accountsQuery.error}
        onRetry={() => accountsQuery.refetch()}
      />
    );
  }

  if (!connected) {
    return (
      <EmptyState
        icon={<Cloud className="w-8 h-8" aria-hidden="true" />}
        title={t('aniListTab.notConnected')}
        description={t('aniListTab.notConnectedHint')}
        action={
          <Button variant="primary" onClick={() => setAuthDialogOpen(true)}>
            {t('aniListTab.connect')}
          </Button>
        }
      />
    );
  }

  if (viewerQuery.isLoading) {
    return (
      <div className="flex flex-col items-center gap-3 py-20 text-xs text-text-muted">
        <Spinner size="lg" label={t('aniListTab.loadingAccount')} />
        <span aria-hidden="true">{t('aniListTab.loadingAccountEllipsis')}</span>
      </div>
    );
  }

  if (viewerQuery.isError || !viewer) {
    return (
      <ErrorState
        title={t('aniListTab.accountError')}
        message={
          viewerQuery.isError
            ? errorMessage(viewerQuery.error)
            : t('aniListTab.noAccount')
        }
        onRetry={() => viewerQuery.refetch()}
        action={
          <Button variant="ghost" onClick={() => setAuthDialogOpen(true)}>
            {t('aniListTab.accounts')}
          </Button>
        }
      />
    );
  }

  const statusLabel =
    ANILIST_STATUS_OPTIONS.find((option) => option.value === status)?.label ?? t('aniListTab.thisList');

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
        <SegmentedControl
          aria-label={t('aniListTab.listLabel')}
          options={ANILIST_STATUS_OPTIONS}
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
    return <ErrorState title={t('aniListTab.listError')} message={error} onRetry={onRetry} />;
  }

  if (entries.length === 0) {
    return (
      <EmptyState
        icon={<ListChecks className="w-8 h-8" aria-hidden="true" />}
        title={t('aniListTab.emptyTitle', { status: statusLabel })}
        description={t('aniListTab.emptyHint')}
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
            <div className="flex items-center justify-between gap-2 ps-0.5 mt-1 text-[11px] text-text-muted tabular-nums">
              <span className="truncate">
                {t('aniListTab.progress', { progress: entry.progress, total: entry.total_episodes ?? '?' })}
                {entry.score > 0 && ` · ${t('aniListTab.score', { score: entry.score })}`}
              </span>
              <Button
                icon
                variant="ghost"
                size="sm"
                aria-label={entry.title ? t('aniListTab.editEntry', { title: entry.title }) : t('aniListTab.editThisEntry')}
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
