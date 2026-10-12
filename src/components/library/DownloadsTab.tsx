import React, { useState } from 'react';
import { Download, Play, RotateCw, Trash2 } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import type { UseQueryResult } from '@tanstack/react-query';
import { Button, EmptyState, ErrorState, Modal, Spinner, errorMessage } from '../ui';
import { useDeleteDownload, useRetryDownload } from '../../hooks/useDownloads';
import { useSettings } from '../../hooks/useSettings';
import { useUIStore } from '../../stores/uiStore';
import type { DownloadRecord } from '../../types';
import {
  categoryLabel,
  downloadStatusLabel,
  episodeLabel,
  externalPlayerFor,
  formatBytes,
  playerLabel,
} from './format';
import { CoverThumb } from './CoverThumb';

interface DownloadsTabProps {
  query: UseQueryResult<DownloadRecord[]>;
}

export const DownloadsTab: React.FC<DownloadsTabProps> = ({ query }) => {
  const { data: settings } = useSettings();
  const showToast = useUIStore((state) => state.showToast);
  const deleteDownload = useDeleteDownload();
  const retryDownload = useRetryDownload();
  const [pendingDelete, setPendingDelete] = useState<DownloadRecord | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const player = externalPlayerFor(settings);

  const handleOpen = async (record: DownloadRecord) => {
    if (!record.file_path) return;
    try {
      await invoke('open_file_in_player', { filePath: record.file_path, player });
    } catch (err) {
      showToast({
        type: 'error',
        title: `Could not open the file in ${playerLabel(player)}`,
        message: errorMessage(err),
      });
    }
  };

  const handleRetry = async (record: DownloadRecord) => {
    try {
      await retryDownload.mutateAsync(record.id);
    } catch (err) {
      showToast({ type: 'error', title: 'Could not retry the download', message: errorMessage(err) });
    }
  };

  const closeDeleteDialog = () => {
    if (deleteDownload.isPending) return;
    setPendingDelete(null);
    setDeleteError(null);
  };

  const confirmDelete = async () => {
    if (!pendingDelete || deleteDownload.isPending) return;
    setDeleteError(null);
    try {
      await deleteDownload.mutateAsync(pendingDelete.id);
      setPendingDelete(null);
    } catch (err) {
      setDeleteError(errorMessage(err));
    }
  };

  if (query.isLoading) {
    return (
      <div className="flex justify-center py-20">
        <Spinner size="lg" label="Loading downloads" className="text-text-muted" />
      </div>
    );
  }

  if (query.isError) {
    return (
      <ErrorState
        title="Could not load downloads"
        error={query.error}
        onRetry={() => query.refetch()}
      />
    );
  }

  const downloads = query.data ?? [];

  if (downloads.length === 0) {
    return (
      <EmptyState
        icon={<Download className="w-8 h-8" aria-hidden="true" />}
        title="No downloads yet"
        description="Queue episodes from an anime's page or from a playlist. They appear here while they download."
      />
    );
  }

  return (
    <>
      <ul className="flex flex-col gap-2">
        {downloads.map((record) => (
          <DownloadRow
            key={record.id}
            record={record}
            playerName={playerLabel(player)}
            onOpen={() => handleOpen(record)}
            onRetry={() => handleRetry(record)}
            retryDisabled={retryDownload.isPending}
            onDelete={() => {
              setDeleteError(null);
              setPendingDelete(record);
            }}
          />
        ))}
      </ul>

      <Modal
        isOpen={pendingDelete !== null}
        onClose={closeDeleteDialog}
        title="Remove this download?"
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={closeDeleteDialog} disabled={deleteDownload.isPending}>
              Cancel
            </Button>
            <Button variant="danger" onClick={confirmDelete} disabled={deleteDownload.isPending}>
              {deleteDownload.isPending ? 'Removing…' : 'Remove'}
            </Button>
          </>
        }
      >
        {pendingDelete && (
          <div className="flex flex-col gap-3 text-sm text-text-secondary">
            <p>
              <span className="text-text-primary">
                {pendingDelete.series_title}, {episodeLabel(pendingDelete.episode_num)}
              </span>{' '}
              will be removed from this list.
            </p>
            {pendingDelete.file_path && (
              <p className="text-xs text-text-muted">
                A finished file stays on disk:
                <span className="block mt-1 select-text break-all">{pendingDelete.file_path}</span>
              </p>
            )}
            {deleteError && (
              <p role="alert" className="text-xs text-state-error select-text">
                {deleteError}
              </p>
            )}
          </div>
        )}
      </Modal>
    </>
  );
};

interface DownloadRowProps {
  record: DownloadRecord;
  playerName: string;
  onOpen: () => void;
  onRetry: () => void;
  retryDisabled: boolean;
  onDelete: () => void;
}

const DownloadRow: React.FC<DownloadRowProps> = ({
  record,
  playerName,
  onOpen,
  onRetry,
  retryDisabled,
  onDelete,
}) => {
  const status = record.status.toUpperCase();
  const fraction = Number.isFinite(record.progress) ? Math.min(1, Math.max(0, record.progress)) : 0;
  const percent = Math.round(fraction * 100);
  const size = formatBytes(record.file_size);
  const name = `${record.series_title}, ${episodeLabel(record.episode_num)}`;

  return (
    <li className="flex items-center gap-3 p-3 rounded-md ui-card bg-bg-card [content-visibility:auto] [contain-intrinsic-size:auto_88px]">
      <CoverThumb src={record.series_cover} className="w-12 h-16 rounded-md flex-shrink-0" />

      <div className="flex flex-col flex-1 min-w-0 gap-1">
        <span className="text-[13px] font-medium text-text-primary truncate">
          {record.series_title}
        </span>
        <span className="text-xs text-text-muted truncate">
          {episodeLabel(record.episode_num)} · {record.quality} · {categoryLabel(record.category)}
        </span>

        {status === 'DOWNLOADING' ? (
          <div className="flex items-center gap-3">
            <div
              role="progressbar"
              aria-label={`Download progress for ${name}`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent}
              className="flex-1 max-w-xs h-1 rounded-full bg-ink/10 overflow-hidden"
            >
              <div className="h-full rounded-full bg-accent-500" style={{ width: `${percent}%` }} />
            </div>
            <span className="text-xs text-text-secondary tabular-nums">
              {percent > 0 ? `Downloading · ${percent}%` : 'Downloading…'}
            </span>
          </div>
        ) : status === 'FAILED' ? (
          <p className="text-xs text-state-error select-text line-clamp-2" title={record.error_msg ?? undefined}>
            Failed{record.error_msg ? `: ${record.error_msg}` : ''}
          </p>
        ) : status === 'COMPLETED' ? (
          <span className="text-xs text-state-success">
            Completed{size ? <span className="text-text-muted"> · {size}</span> : null}
          </span>
        ) : (
          <span className="text-xs text-text-muted">{downloadStatusLabel(record.status)}</span>
        )}
      </div>

      <div className="flex items-center gap-1 flex-shrink-0">
        {status === 'COMPLETED' && record.file_path && (
          <Button
            variant="secondary"
            size="sm"
            onClick={onOpen}
            title={`Open in ${playerName}`}
            aria-label={`Play ${name} in ${playerName}`}
          >
            <Play className="w-3.5 h-3.5 fill-current" aria-hidden="true" />
            <span>Play</span>
          </Button>
        )}
        {status === 'FAILED' && (
          <Button
            variant="secondary"
            size="sm"
            onClick={onRetry}
            disabled={retryDisabled}
            aria-label={`Retry download: ${name}`}
          >
            <RotateCw className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Retry</span>
          </Button>
        )}
        <Button icon variant="ghost" size="sm" onClick={onDelete} aria-label={`Remove download: ${name}`}>
          <Trash2 className="w-4 h-4" aria-hidden="true" />
        </Button>
      </div>
    </li>
  );
};
