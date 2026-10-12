import React, { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { ArrowLeft, Download, ListVideo, Play, Trash2, X } from 'lucide-react';
import { Button, EmptyState, ErrorState, Modal, Spinner, errorMessage } from '../ui';
import {
  launchPlaylistExternalPlayer,
  resolvePlaylistStreams,
  useDeletePlaylist,
  usePlaylistItems,
  useRemoveFromPlaylist,
} from '../../hooks/usePlaylists';
import { newQueuedDownload, useQueueDownloads } from '../../hooks/useDownloads';
import { useSettings } from '../../hooks/useSettings';
import { usePreferredProvider } from '../../hooks/usePlugins';
import { distinctEpisodeTitle, watchPath } from '../../hooks/watchProgress';
import { usePlayerStore } from '../../stores/playerStore';
import { useUIStore } from '../../stores/uiStore';
import type { CustomPlaylist, CustomPlaylistItem } from '../../types';
import { categoryLabel, episodeLabel, formatDate, playerLabel, watchCategory } from './format';
import { CoverThumb } from './CoverThumb';
import { PlaylistCollage } from './PlaylistCollage';

type ExternalPlayer = 'mpv' | 'vlc';

interface ResolveProgress {
  player: ExternalPlayer;
  done: number;
  total: number;
  cancelling: boolean;
}

/** Problems that need more room, and more time on screen, than a toast gives. */
interface IssueReport {
  title: string;
  lines: string[];
  retry?: { label: string; run: () => void };
}

interface PlaylistDetailProps {
  playlist: CustomPlaylist;
  onBack: () => void;
  onDeleted: () => void;
}

function itemName(item: CustomPlaylistItem): string {
  return `${item.series_title}, ${episodeLabel(item.episode_num)}`;
}

function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? '' : 's'}`;
}

export const PlaylistDetail: React.FC<PlaylistDetailProps> = ({ playlist, onBack, onDeleted }) => {
  const navigate = useNavigate();
  const { data: settings } = useSettings();
  // A stored preference that is not an enabled plugin behaves as Auto.
  const preferredProvider = usePreferredProvider(settings?.preferred_provider);
  const setCustomQueue = usePlayerStore((state) => state.setCustomQueue);
  const showToast = useUIStore((state) => state.showToast);

  const itemsQuery = usePlaylistItems(playlist.id);
  const items = itemsQuery.data ?? [];
  const removeItem = useRemoveFromPlaylist();
  const deletePlaylist = useDeletePlaylist();
  const queueDownloads = useQueueDownloads();

  const [resolving, setResolving] = useState<ResolveProgress | null>(null);
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [issues, setIssues] = useState<IssueReport | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  // `isPending` and state only change on the next render; refs are what stop a fast double click.
  const resolvingRef = useRef(false);
  const queueingRef = useRef(false);
  // Stops a running stream lookup from opening a player after Cancel or after leaving the view.
  const cancelRef = useRef(false);
  useEffect(() => {
    cancelRef.current = false;
    return () => {
      cancelRef.current = true;
    };
  }, []);

  const hasItems = items.length > 0;
  const episodeCount = itemsQuery.isSuccess ? items.length : playlist.item_count;
  const created = formatDate(playlist.created_at);

  const handlePlayInApp = (startIndex: number) => {
    if (!hasItems) return;
    const index = Math.max(0, Math.min(items.length - 1, startIndex));
    const target = items[index];
    setCustomQueue(items, index, playlist.name);
    navigate(watchPath(target.anilist_id, target.episode_num, target.category));
  };

  const handleLaunchExternal = async (player: ExternalPlayer) => {
    if (!hasItems || resolvingRef.current) return;
    const name = playerLabel(player);

    resolvingRef.current = true;
    cancelRef.current = false;
    setIssues(null);
    setResolving({ player, done: 0, total: items.length, cancelling: false });

    try {
      const result = await resolvePlaylistStreams(items, {
        provider: preferredProvider,
        onProgress: (done, total) =>
          setResolving((current) => (current ? { ...current, done, total } : current)),
        isCancelled: () => cancelRef.current,
      });
      if (result.cancelled) return;

      const failureLines = result.failures.map(({ item, reason }) => `${itemName(item)}: ${reason}`);

      if (result.resolved.length === 0) {
        setIssues({ title: `No episode could be resolved, so ${name} was not started`, lines: failureLines });
        showToast({ type: 'error', title: 'No streams could be resolved for this playlist' });
        return;
      }

      await launchPlaylistExternalPlayer(player, playlist.name || 'Anix playlist', result.resolved);

      if (result.failures.length > 0) {
        setIssues({
          title: `${plural(result.failures.length, 'episode')} could not be resolved and ${
            result.failures.length === 1 ? 'was' : 'were'
          } left out`,
          lines: failureLines,
        });
        showToast({
          type: 'warning',
          title: `Opened ${result.resolved.length} of ${items.length} episodes in ${name}`,
        });
      } else {
        showToast({
          type: 'success',
          title: `Opened ${plural(result.resolved.length, 'episode')} in ${name}`,
        });
      }
    } catch (err) {
      showToast({ type: 'error', title: `Could not start ${name}`, message: errorMessage(err) });
    } finally {
      resolvingRef.current = false;
      setResolving(null);
    }
  };

  const handleCancelResolve = () => {
    cancelRef.current = true;
    setResolving((current) => (current ? { ...current, cancelling: true } : current));
  };

  const queueItems = async (targets: CustomPlaylistItem[]) => {
    if (targets.length === 0 || queueingRef.current) return;
    queueingRef.current = true;
    setIssues(null);

    const records = targets.map((item) =>
      newQueuedDownload({
        anilistId: item.anilist_id,
        episodeNum: item.episode_num,
        episodeTitle: item.episode_title,
        seriesTitle: item.series_title,
        seriesCover: item.series_cover,
        provider: preferredProvider,
        category: watchCategory(item.category),
        quality: settings?.download_quality || 'best',
      })
    );
    const itemByRecordId = new Map(records.map((record, index) => [record.id, targets[index]]));

    try {
      const result = await queueDownloads.mutateAsync(records);
      const failedItems = result.failures.flatMap(({ record }) => itemByRecordId.get(record.id) ?? []);
      const failed = result.failures.length;

      if (failed > 0) {
        setIssues({
          title: `${plural(failed, 'download')} could not be queued`,
          lines: result.failures.map(
            ({ record, reason }) =>
              `${record.series_title}, ${episodeLabel(record.episode_num)}: ${reason}`
          ),
          // Only the failed ones, so the episodes that were queued are not queued twice.
          retry: { label: `Retry ${failed} failed`, run: () => queueItems(failedItems) },
        });
      }

      if (result.queued === 0) {
        showToast({ type: 'error', title: 'No downloads were queued' });
      } else {
        showToast({
          type: failed > 0 ? 'warning' : 'success',
          title:
            failed > 0
              ? `Queued ${result.queued} of ${records.length} downloads`
              : `Queued ${plural(result.queued, 'download')}`,
          message: 'Track them in the Downloads tab.',
        });
      }
    } catch (err) {
      showToast({ type: 'error', title: 'Could not queue the downloads', message: errorMessage(err) });
    } finally {
      queueingRef.current = false;
    }
  };

  const handleRemoveItem = async (item: CustomPlaylistItem) => {
    if (removingId) return;
    setRemovingId(item.id);
    try {
      await removeItem.mutateAsync({ itemId: item.id, playlistId: playlist.id });
    } catch (err) {
      showToast({
        type: 'error',
        title: 'Could not remove the episode from the playlist',
        message: errorMessage(err),
      });
    } finally {
      setRemovingId(null);
    }
  };

  const closeDeleteDialog = () => {
    if (deletePlaylist.isPending) return;
    setConfirmDelete(false);
    setDeleteError(null);
  };

  const handleDeletePlaylist = async () => {
    if (deletePlaylist.isPending) return;
    setDeleteError(null);
    try {
      await deletePlaylist.mutateAsync(playlist.id);
      showToast({ type: 'success', title: `Deleted playlist "${playlist.name}"` });
      onDeleted();
    } catch (err) {
      setDeleteError(errorMessage(err, 'The playlist could not be deleted.'));
    }
  };

  const externalButton = (player: ExternalPlayer) => {
    const active = resolving?.player === player;
    return (
      <Button
        variant="secondary"
        onClick={() => handleLaunchExternal(player)}
        disabled={!hasItems || resolving !== null}
        title={`Resolve every episode and open the playlist in ${playerLabel(player)}`}
      >
        {active && resolving ? (
          <>
            <Spinner size="sm" label="Resolving streams" />
            <span className="tabular-nums">
              Resolving {Math.min(resolving.done + 1, resolving.total)} of {resolving.total}
            </span>
          </>
        ) : (
          <span>Play in {playerLabel(player)}</span>
        )}
      </Button>
    );
  };

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between gap-3">
        <Button variant="ghost" size="sm" className="-ml-3" onClick={onBack}>
          <ArrowLeft className="w-4 h-4" aria-hidden="true" />
          <span>All playlists</span>
        </Button>
        <Button variant="danger" size="sm" onClick={() => setConfirmDelete(true)}>
          <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
          <span>Delete playlist</span>
        </Button>
      </div>

      <header className="flex items-center gap-5 min-w-0">
        <PlaylistCollage covers={playlist.covers} className="w-24 h-24 rounded-xl flex-shrink-0" />
        <div className="min-w-0">
          <p className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Playlist</p>
          <h2 className="mt-1 text-xl font-semibold text-text-primary truncate select-text">
            {playlist.name}
          </h2>
          {playlist.description && (
            <p className="mt-1 max-w-xl text-[13px] text-text-secondary line-clamp-2 select-text">
              {playlist.description}
            </p>
          )}
          <p className="mt-2 text-xs text-text-muted">
            {plural(episodeCount, 'episode')}
            {created ? ` · Created ${created}` : ''}
          </p>
        </div>
      </header>

      <div className="flex flex-wrap items-center gap-2">
        <Button variant="primary" onClick={() => handlePlayInApp(0)} disabled={!hasItems}>
          <Play className="w-4 h-4 fill-current" aria-hidden="true" />
          <span>Play</span>
        </Button>
        {externalButton('mpv')}
        {externalButton('vlc')}
        {resolving && (
          <Button variant="ghost" onClick={handleCancelResolve} disabled={resolving.cancelling}>
            {resolving.cancelling ? 'Cancelling…' : 'Cancel'}
          </Button>
        )}
        <Button
          variant="secondary"
          onClick={() => queueItems(items)}
          disabled={!hasItems || queueDownloads.isPending}
        >
          {queueDownloads.isPending ? (
            <>
              <Spinner size="sm" label="Queuing downloads" />
              <span>Queuing…</span>
            </>
          ) : (
            <>
              <Download className="w-4 h-4" aria-hidden="true" />
              <span>Download all</span>
            </>
          )}
        </Button>
      </div>

      {issues && (
        <div role="alert" className="flex items-start gap-3 p-4 rounded-lg bg-state-error/10">
          <div className="flex-1 min-w-0">
            <p className="text-[13px] font-medium text-state-error">{issues.title}</p>
            {issues.lines.length > 0 && (
              <ul className="mt-2 flex flex-col gap-1 max-h-40 overflow-y-auto text-xs text-text-secondary select-text">
                {issues.lines.map((line, index) => (
                  <li key={`${index}-${line}`} className="break-words">
                    {line}
                  </li>
                ))}
              </ul>
            )}
            {issues.retry && (
              <Button
                variant="secondary"
                size="sm"
                className="mt-3"
                onClick={issues.retry.run}
                disabled={queueDownloads.isPending}
              >
                {issues.retry.label}
              </Button>
            )}
          </div>
          <Button icon variant="ghost" size="sm" aria-label="Dismiss" onClick={() => setIssues(null)}>
            <X className="w-4 h-4" aria-hidden="true" />
          </Button>
        </div>
      )}

      {itemsQuery.isLoading ? (
        <div className="flex justify-center py-16">
          <Spinner size="lg" label="Loading playlist" className="text-text-muted" />
        </div>
      ) : itemsQuery.isError ? (
        <ErrorState
          title="Could not load this playlist"
          error={itemsQuery.error}
          onRetry={() => itemsQuery.refetch()}
        />
      ) : !hasItems ? (
        <EmptyState
          icon={<ListVideo className="w-8 h-8" aria-hidden="true" />}
          title="This playlist is empty"
          description="Open an anime and use Add to playlist on an episode to put it here."
        />
      ) : (
        <ol className="flex flex-col gap-2">
          {items.map((item, index) => {
            const title = distinctEpisodeTitle(item.episode_num, item.episode_title);

            return (
              <li
                key={item.id}
                className="flex items-center gap-3 p-2 pr-3 rounded-md ui-card bg-bg-card [content-visibility:auto] [contain-intrinsic-size:auto_80px]"
              >
                <span className="w-6 flex-shrink-0 text-center text-xs text-text-muted tabular-nums">
                  {index + 1}
                </span>
                <CoverThumb src={item.series_cover} className="w-11 h-16 rounded-md flex-shrink-0" />
                <div className="flex flex-col flex-1 min-w-0 gap-0.5">
                  <span className="text-[13px] font-medium text-text-primary truncate">
                    {item.series_title}
                  </span>
                  <span className="text-xs text-text-muted truncate">
                    {episodeLabel(item.episode_num)} · {categoryLabel(item.category)}
                    {title ? ` · ${title}` : ''}
                  </span>
                </div>
                <div className="flex items-center gap-1 flex-shrink-0">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => handlePlayInApp(index)}
                    aria-label={`Play from ${itemName(item)}`}
                  >
                    <Play className="w-3.5 h-3.5 fill-current" aria-hidden="true" />
                    <span>Play</span>
                  </Button>
                  <Button
                    icon
                    variant="ghost"
                    size="sm"
                    onClick={() => handleRemoveItem(item)}
                    disabled={removingId !== null}
                    aria-label={`Remove ${itemName(item)} from the playlist`}
                  >
                    <Trash2 className="w-4 h-4" aria-hidden="true" />
                  </Button>
                </div>
              </li>
            );
          })}
        </ol>
      )}

      <Modal
        isOpen={confirmDelete}
        onClose={closeDeleteDialog}
        title="Delete this playlist?"
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={closeDeleteDialog} disabled={deletePlaylist.isPending}>
              Cancel
            </Button>
            <Button variant="danger" onClick={handleDeletePlaylist} disabled={deletePlaylist.isPending}>
              {deletePlaylist.isPending ? 'Deleting…' : 'Delete playlist'}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3 text-sm text-text-secondary">
          <p>
            <span className="text-text-primary">{playlist.name}</span>
            {episodeCount > 0 ? ` and its list of ${plural(episodeCount, 'episode')}` : ''} will be
            deleted. Watch history and downloads are not affected. This cannot be undone.
          </p>
          {deleteError && (
            <p role="alert" className="text-xs text-state-error select-text">
              {deleteError}
            </p>
          )}
        </div>
      </Modal>
    </div>
  );
};
