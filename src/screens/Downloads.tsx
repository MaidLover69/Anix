import React, { useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Compass, Download, FolderOpen, Play, RotateCw, Trash2, X } from 'lucide-react';
import clsx from 'clsx';
import { invoke } from '@tauri-apps/api/core';
import { Button, EmptyState, ErrorState, Modal, Spinner, Switch, errorMessage } from '../components/ui';
import {
  useAllDownloads,
  useCheckStorage,
  useDeleteDownload,
  useDownloadFolder,
  useRetryDownload,
} from '../hooks/useDownloads';
import { usePlayful, useSettings } from '../hooks/useSettings';
import { useUIStore } from '../stores/uiStore';
import { downloadLanguageLabel } from '../lib/downloadPicks';
import type { DownloadRecord } from '../types';
import { CoverThumb } from '../components/library/CoverThumb';
import { episodeLabel, externalPlayerFor, formatBytes, playerLabel } from '../components/library/format';
import { t, tn } from '../i18n';

const fractionOf = (record: DownloadRecord) =>
  Number.isFinite(record.progress) ? Math.min(1, Math.max(0, record.progress)) : 0;

const rowName = (record: DownloadRecord) =>
  t('downloads.rowName', { title: record.series_title, episode: episodeLabel(record.episode_num) });

const sizeOrZero = (bytes: number) => formatBytes(bytes) || t('downloads.zeroSize');

const details = (record: DownloadRecord) =>
  [episodeLabel(record.episode_num), record.quality, downloadLanguageLabel(record)].join(' · ');

/** A poster with a progress ring over it while the episode is queued or downloading. */
const PosterWithRing: React.FC<{ record: DownloadRecord; size?: 'sm' | 'lg' }> = ({ record, size = 'sm' }) => {
  const status = record.status.toUpperCase();
  const active = status === 'DOWNLOADING' || status === 'QUEUED';
  const percent = Math.round(fractionOf(record) * 100);
  const radius = 15;
  const circumference = 2 * Math.PI * radius;
  return (
    <div className={clsx('relative flex-shrink-0', size === 'lg' ? 'w-16 h-24' : 'w-12 h-16')}>
      <CoverThumb src={record.series_cover} className="h-full w-full rounded-md" />
      {active && (
        <div aria-hidden="true" className="absolute inset-0 flex items-center justify-center rounded-md bg-bg-deepest/60">
          <svg viewBox="0 0 36 36" className="h-9 w-9 -rotate-90">
            <circle cx="18" cy="18" r={radius} fill="none" strokeWidth="3" className="stroke-ink/20" />
            <circle
              cx="18"
              cy="18"
              r={radius}
              fill="none"
              strokeWidth="3"
              strokeLinecap="round"
              strokeDasharray={circumference}
              strokeDashoffset={circumference * (1 - percent / 100)}
              className="stroke-accent-500 motion-safe:transition-[stroke-dashoffset]"
            />
          </svg>
          <span className="absolute text-[10px] font-semibold tabular-nums text-text-primary">
            {status === 'QUEUED' ? '…' : `${percent}%`}
          </span>
        </div>
      )}
    </div>
  );
};

/** Disk usage of the download folder's disk, with Change folder. */
const DiskUsage: React.FC<{ savedBytes: number }> = ({ savedBytes }) => {
  const showToast = useUIStore((s) => s.showToast);
  const storage = useCheckStorage('720p').data;
  const { change } = useDownloadFolder();
  const total = storage?.total_bytes ?? 0;
  const free = storage?.free_bytes ?? 0;
  const used = Math.max(0, total - free);
  const ours = Math.min(savedBytes, used);
  const pct = (bytes: number) => (total > 0 ? (bytes / total) * 100 : 0);

  return (
    <section aria-label={t('downloads.diskUsage')} className="flex flex-col gap-3 rounded-xl ui-card bg-bg-card p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-text-primary">{t('downloads.downloadFolder')}</p>
          <p className="mt-0.5 truncate text-xs text-text-muted select-text" title={storage?.dir}>
            {storage?.dir ?? t('downloads.reading')}
          </p>
        </div>
        <Button
          variant="secondary"
          size="sm"
          disabled={change.isPending}
          onClick={() =>
            change.mutate(undefined, {
              onSuccess: (picked) =>
                picked && showToast({ type: 'success', title: t('downloads.newDownloadsGoHere'), message: picked }),
              onError: (err) =>
                showToast({ type: 'error', title: t('downloads.couldNotUseFolder'), message: errorMessage(err) }),
            })
          }
        >
          <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
          {t('downloads.changeFolder')}
        </Button>
      </div>
      {total > 0 && (
        <>
          <div
            role="meter"
            aria-label={t('downloads.diskUsage')}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(pct(used))}
            aria-valuetext={t('downloads.freeOf', { free: sizeOrZero(free), total: formatBytes(total) })}
            className="flex h-2 w-full overflow-hidden rounded-full bg-ink/10"
          >
            <div className="h-full bg-accent-500" style={{ width: `${pct(ours)}%` }} />
            <div className="h-full bg-ink/30" style={{ width: `${pct(used - ours)}%` }} />
          </div>
          <p className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-text-muted">
            <span className="flex items-center gap-1.5">
              <span aria-hidden="true" className="h-2 w-2 rounded-full bg-accent-500" />
              {t('downloads.ofDownloads', { size: sizeOrZero(savedBytes) })}
            </span>
            <span className="flex items-center gap-1.5">
              <span aria-hidden="true" className="h-2 w-2 rounded-full bg-ink/30" />
              {t('downloads.otherFiles', { size: sizeOrZero(used - ours) })}
            </span>
            <span>{t('downloads.freeOf', { free: sizeOrZero(free), total: formatBytes(total) })}</span>
          </p>
        </>
      )}
    </section>
  );
};

/** The download running now, larger, at the top. */
const CurrentBanner: React.FC<{ record: DownloadRecord }> = ({ record }) => {
  const percent = Math.round(fractionOf(record) * 100);
  return (
    <section aria-label={t('downloads.downloadingNow')} className="flex items-center gap-4 rounded-xl ui-card bg-accent-tint p-4">
      <PosterWithRing record={record} size="lg" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="text-[11px] font-semibold uppercase tracking-wider text-accent-300">{t('downloads.downloadingNow')}</p>
        <p className="truncate text-sm font-semibold text-text-primary">{record.series_title}</p>
        <p className="truncate text-xs text-text-secondary">{details(record)}</p>
        <div
          role="progressbar"
          aria-label={t('downloads.progressFor', { name: rowName(record) })}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          className="mt-1 h-1.5 w-full max-w-md overflow-hidden rounded-full bg-ink/10"
        >
          <div className="h-full rounded-full bg-accent-500" style={{ width: `${percent}%` }} />
        </div>
        <p className="text-xs tabular-nums text-text-muted">
          {percent > 0 ? `${percent}%` : t('downloads.starting')}
          {record.file_size ? ` · ${formatBytes(record.file_size)}` : ''}
        </p>
      </div>
    </section>
  );
};

const Group: React.FC<{ title: string; count: number; action?: React.ReactNode; children: React.ReactNode }> = ({
  title,
  count,
  action,
  children,
}) => {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-4">
        <h2 id={id} className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
          {title} <span className="tabular-nums">({count})</span>
        </h2>
        {action}
      </div>
      <ul className="flex flex-col gap-2">{children}</ul>
    </section>
  );
};

const Row: React.FC<{ record: DownloadRecord; children?: React.ReactNode; tone?: 'error' }> = ({
  record,
  children,
  tone,
}) => {
  const status = record.status.toUpperCase();
  const size = formatBytes(record.file_size);
  return (
    <li className="flex items-center gap-3 rounded-md ui-card bg-bg-card p-3 [content-visibility:auto] [contain-intrinsic-size:auto_88px]">
      <PosterWithRing record={record} />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="truncate text-[13px] font-medium text-text-primary">{record.series_title}</span>
        <span className="truncate text-xs text-text-muted">{details(record)}</span>
        {tone === 'error' ? (
          <p className="line-clamp-2 text-xs text-state-error select-text" title={record.error_msg ?? undefined}>
            {record.error_msg || t('downloads.failedFallback')}
          </p>
        ) : status === 'COMPLETED' ? (
          <span className="text-xs text-state-success">{t('downloads.saved')}{size ? <span className="text-text-muted"> · {size}</span> : null}</span>
        ) : status === 'QUEUED' ? (
          <span className="text-xs text-text-muted">{t('downloads.waiting')}</span>
        ) : (
          <span className="text-xs tabular-nums text-text-secondary">
            {t('downloads.downloadingPercent', { percent: Math.round(fractionOf(record) * 100) })}
          </span>
        )}
      </div>
      <div className="flex flex-shrink-0 items-center gap-1">{children}</div>
    </li>
  );
};

export const Downloads: React.FC = () => {
  const navigate = useNavigate();
  const playful = usePlayful();
  const { data: settings } = useSettings();
  const showToast = useUIStore((s) => s.showToast);
  const query = useAllDownloads({ watch: true });
  const deleteDownload = useDeleteDownload();
  const retryDownload = useRetryDownload();
  const [pendingDelete, setPendingDelete] = useState<DownloadRecord | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleteFile, setDeleteFile] = useState(true);
  const deleteFileLabel = useId();
  const canDeleteFile = !!pendingDelete?.file_path && pendingDelete.status.toUpperCase() === 'COMPLETED';
  const player = externalPlayerFor(settings);

  const downloads = query.data ?? [];
  const byStatus = (status: string) => downloads.filter((d) => d.status.toUpperCase() === status);
  const running = byStatus('DOWNLOADING');
  const queued = byStatus('QUEUED').sort((a, b) => a.created_at - b.created_at);
  const failed = byStatus('FAILED');
  const saved = byStatus('COMPLETED');
  const savedBytes = saved.reduce((sum, d) => sum + (d.file_size ?? 0), 0);

  const play = async (record: DownloadRecord) => {
    if (!record.file_path) return;
    try {
      await invoke('open_file_in_player', { filePath: record.file_path, player });
    } catch (err) {
      showToast({
        type: 'error',
        title: t('downloads.couldNotOpenIn', { player: playerLabel(player) }),
        message: errorMessage(err),
      });
    }
  };

  const retry = (record: DownloadRecord) =>
    retryDownload.mutate(record.id, {
      onError: (err) => showToast({ type: 'error', title: t('downloads.couldNotTryAgain'), message: errorMessage(err) }),
    });

  const forget = (record: DownloadRecord) =>
    deleteDownload.mutate({ id: record.id }, {
      onError: (err) => showToast({ type: 'error', title: t('downloads.couldNotRemove'), message: errorMessage(err) }),
    });

  const closeDeleteDialog = () => {
    if (deleteDownload.isPending) return;
    setPendingDelete(null);
    setDeleteError(null);
  };

  const confirmDelete = () => {
    if (!pendingDelete || deleteDownload.isPending) return;
    setDeleteError(null);
    deleteDownload.mutate({ id: pendingDelete.id, deleteFile: canDeleteFile && deleteFile }, {
      onSuccess: () => setPendingDelete(null),
      onError: (err) => setDeleteError(errorMessage(err)),
    });
  };

  const removeButton = (record: DownloadRecord) => (
    <Button
      icon
      variant="ghost"
      size="sm"
      aria-label={t('downloads.removeFor', { name: rowName(record) })}
      onClick={() => {
        setDeleteError(null);
        setDeleteFile(true);
        setPendingDelete(record);
      }}
    >
      <Trash2 className="h-4 w-4" aria-hidden="true" />
    </Button>
  );

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <div>
        <h1 className="text-xl font-semibold text-text-primary">{t('downloads.title')}</h1>
        {downloads.length > 0 && (
          <p className="mt-1 text-xs text-text-muted">
            {[
              running.length + queued.length > 0
                ? t('downloads.inQueue', { count: running.length + queued.length })
                : null,
              saved.length > 0 ? t('downloads.savedCount', { count: saved.length }) : null,
              failed.length > 0 ? t('downloads.failedCount', { count: failed.length }) : null,
            ]
              .filter(Boolean)
              .join(' · ')}
          </p>
        )}
      </div>

      <DiskUsage savedBytes={savedBytes} />

      {query.isLoading ? (
        <div className="flex justify-center py-20">
          <Spinner size="lg" label={t('downloads.loading')} className="text-text-muted" />
        </div>
      ) : query.isError ? (
        <ErrorState title={t('downloads.couldNotLoad')} error={query.error} onRetry={() => query.refetch()} />
      ) : downloads.length === 0 ? (
        <EmptyState
          icon={<Download aria-hidden="true" />}
          title={playful ? t('downloads.nothingDownloadedYet') : t('downloads.noDownloadsYet')}
          description={playful ? t('downloads.playfulEmpty') : t('downloads.emptyDescription')}
          mood="sleepy"
          kaomoji="(=^_^=)" /* i18n-ignore */
          action={
            <Button variant="primary" onClick={() => navigate('/discover')}>
              <Compass aria-hidden="true" className="h-4 w-4" />
              {t('downloads.browseAnime')}
            </Button>
          }
        />
      ) : (
        <>
          {running[0] && <CurrentBanner record={running[0]} />}

          {running.length > 1 && (
            <Group title={t('downloads.alsoDownloading')} count={running.length - 1}>
              {running.slice(1).map((record) => (
                <Row key={record.id} record={record}>
                  {removeButton(record)}
                </Row>
              ))}
            </Group>
          )}

          {queued.length > 0 && (
            <Group title={t('downloads.upNext')} count={queued.length}>
              {queued.map((record) => (
                <Row key={record.id} record={record}>
                  {removeButton(record)}
                </Row>
              ))}
            </Group>
          )}

          {failed.length > 0 && (
            <Group title={t('downloads.failed')} count={failed.length}>
              {failed.map((record) => (
                <Row key={record.id} record={record} tone="error">
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={retryDownload.isPending}
                    onClick={() => retry(record)}
                    aria-label={t('downloads.tryAgainFor', { name: rowName(record) })}
                  >
                    <RotateCw className="h-3.5 w-3.5" aria-hidden="true" />
                    <span>{t('common.tryAgain')}</span>
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={deleteDownload.isPending}
                    onClick={() => forget(record)}
                    aria-label={t('downloads.forgetFor', { name: rowName(record) })}
                  >
                    <X className="h-3.5 w-3.5" aria-hidden="true" />
                    <span>{t('downloads.forget')}</span>
                  </Button>
                </Row>
              ))}
            </Group>
          )}

          {saved.length > 0 && (
            <Group title={t('downloads.saved')} count={saved.length}>
              {saved.map((record) => (
                <Row key={record.id} record={record}>
                  {record.file_path && (
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => play(record)}
                      title={t('downloads.openIn', { player: playerLabel(player) })}
                      aria-label={t('downloads.playIn', { name: rowName(record), player: playerLabel(player) })}
                    >
                      <Play className="h-3.5 w-3.5 fill-current" aria-hidden="true" />
                      <span>{t('common.play')}</span>
                    </Button>
                  )}
                  {removeButton(record)}
                </Row>
              ))}
            </Group>
          )}
        </>
      )}

      <Modal
        isOpen={pendingDelete !== null}
        onClose={closeDeleteDialog}
        title={t('downloads.removeTitle')}
        size="md"
        footer={
          <>
            <Button variant="ghost" onClick={closeDeleteDialog} disabled={deleteDownload.isPending}>
              {t('common.cancel')}
            </Button>
            <Button variant="danger" onClick={confirmDelete} disabled={deleteDownload.isPending}>
              {deleteDownload.isPending
                ? t('downloads.removing')
                : canDeleteFile && deleteFile
                  ? t('common.delete')
                  : t('common.remove')}
            </Button>
          </>
        }
      >
        {pendingDelete && (
          <div className="flex flex-col gap-3 text-sm text-text-secondary">
            <p>
              {tn(
                pendingDelete.status.toUpperCase() === 'COMPLETED'
                  ? 'downloads.willBeRemoved'
                  : 'downloads.willBeRemovedAndStopped',
                { name: <span className="text-text-primary">{rowName(pendingDelete)}</span> }
              )}
            </p>
            {canDeleteFile && (
              <label className="flex items-center justify-between gap-3 text-text-primary">
                <span id={deleteFileLabel}>{t('downloads.deleteFileToo')}</span>
                <Switch aria-labelledby={deleteFileLabel} checked={deleteFile} onChange={setDeleteFile} />
              </label>
            )}
            {pendingDelete.file_path && (
              <p className="text-xs text-text-muted">
                {canDeleteFile && deleteFile ? t('downloads.fileWillBeDeleted') : t('downloads.fileStaysOnDisk')}
                <span className="mt-1 block break-all select-text">{pendingDelete.file_path}</span>
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
    </div>
  );
};
