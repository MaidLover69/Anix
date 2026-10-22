import React, { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  AlertCircle,
  Check,
  ChevronDown,
  ExternalLink,
  Film,
  FolderOpen,
  FolderSearch,
  Link2,
  Loader2,
  Play,
  RefreshCw,
} from 'lucide-react';
import clsx from 'clsx';
import { invoke } from '@tauri-apps/api/core';
import { Button, Chip, EmptyState, ErrorState, Spinner, errorMessage } from '../ui';
import { useSettings } from '../../hooks/useSettings';
import { useProgressiveList } from '../../hooks/useProgressiveList';
import type { LocalScan } from '../../hooks/useLocalScan';
import { LOCAL_PROVIDER, pickLocalMedia } from '../../hooks/useLocalMedia';
import {
  candidateTitle,
  confirmSeriesMatch,
  episodeFiles,
  matchRequest,
  useLinkSeries,
  useSeriesMatching,
  type LibraryFile,
  type LocalSeries,
  type MatchCandidate,
  type MatchingState,
  type SeriesMatch,
} from '../../hooks/useLocalLibrary';
import { watchPath } from '../../hooks/watchProgress';
import { useUIStore } from '../../stores/uiStore';
import { CoverThumb } from './CoverThumb';
import { INPUT_CLASS, episodeLabel, externalPlayerFor, formatBytes, playerLabel } from './format';
import { ShowMore } from './PosterGrid';
import { LocalMatchDialog, candidateDetails } from './LocalMatchDialog';
import { t, tn, type TKey } from '../../i18n';

interface LocalFilesTabProps {
  scan: LocalScan;
}

type Filter = 'all' | 'review' | 'linked';

const KIND_LABELS: Record<string, TKey> = {
  movie: 'localFilesTab.kindMovie',
  ova: 'localFilesTab.kindOva',
  ona: 'localFilesTab.kindOna',
  special: 'localFilesTab.kindSpecials',
};

/** "1–12, 14, 24.5" */
export function episodeRanges(episodes: number[]): string {
  const parts: string[] = [];
  let start: number | null = null;
  let prev: number | null = null;
  const flush = () => {
    if (start === null || prev === null) return;
    parts.push(start === prev ? String(start) : `${start}–${prev}`);
  };
  for (const n of episodes) {
    const whole = Number.isInteger(n);
    if (start !== null && prev !== null && whole && Number.isInteger(prev) && n === prev + 1) {
      prev = n;
      continue;
    }
    flush();
    start = n;
    prev = n;
  }
  flush();
  return parts.join(', ');
}

function linkedIds(series: LocalSeries, scan: LocalScan): number[] {
  const ids = [...series.linked_anilist_ids];
  for (const id of scan.linked[series.key] ?? []) if (!ids.includes(id)) ids.push(id);
  return ids;
}

export const LocalFilesTab: React.FC<LocalFilesTabProps> = ({ scan }) => {
  const { data: settings } = useSettings();
  const showToast = useUIStore((state) => state.showToast);
  const player = externalPlayerFor(settings);
  const isScanning = scan.status === 'scanning';
  const library = scan.library;
  const matching = useSeriesMatching(library, scan.matches, scan.setMatches);
  // A folder chosen in the picker is scanned once the typed folder has taken its value.
  const [scanPicked, setScanPicked] = useState(false);
  const { scan: runScan } = scan;
  useEffect(() => {
    if (!scanPicked) return;
    setScanPicked(false);
    runScan();
  }, [scanPicked, runScan]);
  const chooseFolder = async () => {
    try {
      const [folder] = await pickLocalMedia(false);
      if (!folder) return;
      scan.setDir(folder);
      setScanPicked(true);
    } catch (err) {
      showToast({ type: 'error', title: t('localFilesTab.pickerError'), message: errorMessage(err) });
    }
  };

  const handlePlay = async (file: LibraryFile) => {
    try {
      await invoke('open_file_in_player', { filePath: file.file_path, player });
    } catch (err) {
      showToast({
        type: 'error',
        title: t('localFilesTab.openFileError', { player: playerLabel(player) }),
        message: errorMessage(err),
      });
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <form
        className="flex flex-col gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          scan.scan();
        }}
      >
        <label htmlFor="local-scan-dir" className="text-[13px] font-medium text-text-primary">
          {t('localFilesTab.folderToScan')}
        </label>
        <div className="flex items-center gap-2">
          <input
            id="local-scan-dir"
            type="text"
            value={scan.dir}
            onChange={(event) => scan.setDir(event.target.value)}
            disabled={isScanning}
            placeholder={t('localFilesTab.dirPlaceholder')}
            spellCheck={false}
            autoComplete="off"
            className={`${INPUT_CLASS} flex-1 min-w-0 max-w-xl`}
          />
          <Button type="submit" variant="primary" disabled={isScanning}>
            {isScanning ? t('localFilesTab.scanning') : t('localFilesTab.scanFolder')}
          </Button>
        </div>
        <p className="text-xs text-text-muted">
          {t('localFilesTab.scanHint')}
        </p>
      </form>

      {isScanning ? (
        <div className="flex flex-col items-center gap-3 py-16 text-center">
          <Spinner size="lg" label={t('localFilesTab.scanningFolder')} className="text-text-muted" />
          <p className="text-[13px] text-text-secondary">
            {tn('localFilesTab.scanningDir', { folder: <span className="select-text break-all">{scan.scannedDir}</span> })}
          </p>
          <p className="text-xs text-text-muted">{t('localFilesTab.scanSlow')}</p>
        </div>
      ) : scan.status === 'error' ? (
        <ErrorState
          title={t('localFilesTab.scanError')}
          message={scan.error ?? t('localFilesTab.unknownError')}
          onRetry={() => scan.scan()}
        />
      ) : scan.status === 'idle' || !library ? (
        <EmptyState
          icon={<FolderSearch className="w-8 h-8" aria-hidden="true" />}
          title={t('localFilesTab.noScan')}
          description={t('localFilesTab.noScanHint')}
          action={
            <Button variant="primary" onClick={chooseFolder}>
              <FolderOpen className="h-4 w-4" aria-hidden="true" />
              {t('localFilesTab.chooseFolder')}
            </Button>
          }
        />
      ) : library.file_count === 0 ? (
        <EmptyState
          icon={<Film className="w-8 h-8" aria-hidden="true" />}
          title={t('localFilesTab.noFiles')}
          description={
            tn('localFilesTab.noFilesHint', { folder: <span className="select-text break-all">{scan.scannedDir}</span> })
          }
          action={
            <Button variant="secondary" onClick={chooseFolder}>
              <FolderOpen className="h-4 w-4" aria-hidden="true" />
              {t('localFilesTab.chooseAnother')}
            </Button>
          }
        />
      ) : (
        <SeriesList
          scan={scan}
          matching={matching.state}
          onRetryMatching={matching.retry}
          playerName={playerLabel(player)}
          onPlayFile={handlePlay}
        />
      )}
    </div>
  );
};

const MatchingStatus: React.FC<{ state: MatchingState; onRetry: () => void }> = ({ state, onRetry }) => {
  if (state.phase === 'idle') return null;
  if (state.phase === 'error') {
    return (
      <div role="alert" className="flex flex-wrap items-center gap-2 text-xs text-state-error">
        <AlertCircle className="h-3.5 w-3.5" aria-hidden="true" />
        <span className="select-text">{t('localFilesTab.matchingStopped', { message: state.message })}</span>
        <Button size="sm" variant="ghost" onClick={onRetry}>
          <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
          <span>{t('common.tryAgain')}</span>
        </Button>
      </div>
    );
  }
  return (
    <div role="status" className="flex items-center gap-2 text-xs text-text-muted tabular-nums">
      <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
      {state.phase === 'waiting'
        ? t('localFilesTab.matchingWaiting', { seconds: state.seconds, done: state.done, total: state.total })
        : t('localFilesTab.matchingProgress', { done: state.done, total: state.total })}
    </div>
  );
};

interface SeriesListProps {
  scan: LocalScan;
  matching: MatchingState;
  onRetryMatching: () => void;
  playerName: string;
  onPlayFile: (file: LibraryFile) => void;
}

const SeriesList: React.FC<SeriesListProps> = ({ scan, matching, onRetryMatching, playerName, onPlayFile }) => {
  const library = scan.library!;
  const showToast = useUIStore((state) => state.showToast);
  const navigate = useNavigate();
  const linkSeries = useLinkSeries();
  const [filter, setFilter] = useState<Filter>('all');
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [picking, setPicking] = useState<LocalSeries | null>(null);

  const counts = useMemo(() => {
    let review = 0;
    let linked = 0;
    for (const series of library.series) {
      if (linkedIds(series, scan).length > 0) linked += 1;
      else if (series.matchable && scan.matches[series.key]?.status !== 'confirmed') review += 1;
    }
    return { review, linked };
  }, [library, scan]);

  const visible = useMemo(() => {
    return library.series.filter((series) => {
      const isLinked = linkedIds(series, scan).length > 0;
      if (filter === 'linked') return isLinked;
      if (filter === 'review') {
        return !isLinked && series.matchable && scan.matches[series.key]?.status !== 'confirmed';
      }
      return true;
    });
  }, [library, scan, filter]);

  const list = useProgressiveList(visible, 40, `${library.root}|${filter}`);

  const pick = async (series: LocalSeries, candidate: MatchCandidate) => {
    setBusyKey(series.key);
    try {
      scan.setMatches([await confirmSeriesMatch(series.key, candidate)]);
      setPicking(null);
    } catch (err) {
      showToast({ type: 'error', title: t('localFilesTab.saveMatchError'), message: errorMessage(err) });
    } finally {
      setBusyKey(null);
    }
  };

  const research = async (series: LocalSeries) => {
    setBusyKey(series.key);
    try {
      const batch = await invoke<{ matches: SeriesMatch[]; retry_after_s: number | null; error: string | null }>(
        'match_local_series',
        { requests: [matchRequest(series)], force: true }
      );
      if (batch.matches.length) scan.setMatches(batch.matches);
      if (batch.retry_after_s) {
        showToast({
          type: 'warning',
          title: t('localFilesTab.slowDown'),
          message: t('localFilesTab.tryAgainIn', { seconds: batch.retry_after_s }),
        });
      } else if (batch.error) {
        showToast({ type: 'error', title: t('localFilesTab.searchFailed'), message: batch.error });
      }
    } catch (err) {
      showToast({ type: 'error', title: t('localFilesTab.searchFailed'), message: errorMessage(err) });
    } finally {
      setBusyKey(null);
    }
  };

  const link = async (series: LocalSeries, candidate: MatchCandidate) => {
    setBusyKey(series.key);
    try {
      const current = scan.matches[series.key];
      if (current?.status !== 'confirmed' || current.media?.id !== candidate.id) {
        scan.setMatches([await confirmSeriesMatch(series.key, candidate)]);
      }
      const result = await linkSeries(series, candidate.id);
      scan.markLinked(series.key, candidate.id);
      const count = result.episodes.length;
      showToast({
        type: 'success',
        title: t('localFilesTab.linkedTo', { title: candidateTitle(candidate) }),
        message: t('localFilesTab.linkedEpisodes', { count }),
      });
    } catch (err) {
      showToast({ type: 'error', title: t('localFilesTab.linkError'), message: errorMessage(err) });
    } finally {
      setBusyKey(null);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs text-text-muted">
          {t('localFilesTab.seriesCount', { count: library.series.length })} ·{' '}
          {tn(
            'localFilesTab.filesIn',
            { folder: <span className="select-text break-all">{library.root}</span> },
            { count: library.file_count }
          )}
        </p>
        <div className="flex items-center gap-1.5" role="group" aria-label={t('localFilesTab.show')}>
          <Chip selected={filter === 'all'} onClick={() => setFilter('all')}>
            {t('localFilesTab.all')}
          </Chip>
          <Chip selected={filter === 'review'} onClick={() => setFilter('review')}>
            {t('localFilesTab.toReview', { count: counts.review })}
          </Chip>
          <Chip selected={filter === 'linked'} onClick={() => setFilter('linked')}>
            {t('localFilesTab.linkedCount', { count: counts.linked })}
          </Chip>
        </div>
      </div>

      <MatchingStatus state={matching} onRetry={onRetryMatching} />

      {visible.length === 0 ? (
        <p className="py-10 text-center text-xs text-text-muted">{t('localFilesTab.filterEmpty')}</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {list.visible.map((series) => (
            <SeriesCard
              key={series.key || 'unrecognised'}
              series={series}
              match={scan.matches[series.key]}
              linked={linkedIds(series, scan)}
              busy={busyKey === series.key}
              playerName={playerName}
              onPlayFile={onPlayFile}
              onChange={() => setPicking(series)}
              onLink={(candidate) => link(series, candidate)}
              onOpen={(id) => navigate(`/detail/${id}`)}
              onPlay={(id, episode) => navigate(watchPath(id, episode, 'sub', LOCAL_PROVIDER))}
            />
          ))}
        </ul>
      )}

      {list.hasMore && (
        <ShowMore
          sentinelRef={list.sentinelRef}
          onClick={list.showMore}
          remaining={visible.length - list.visible.length}
        />
      )}

      <LocalMatchDialog
        series={picking}
        match={picking ? scan.matches[picking.key] : undefined}
        onClose={() => setPicking(null)}
        onPick={pick}
        onResearch={research}
        busy={picking !== null && busyKey === picking.key}
      />
    </div>
  );
};

interface SeriesCardProps {
  series: LocalSeries;
  match: SeriesMatch | undefined;
  linked: number[];
  busy: boolean;
  playerName: string;
  onPlayFile: (file: LibraryFile) => void;
  onChange: () => void;
  onLink: (candidate: MatchCandidate) => void;
  onOpen: (anilistId: number) => void;
  onPlay: (anilistId: number, episode: number) => void;
}

const SeriesCard: React.FC<SeriesCardProps> = ({
  series,
  match,
  linked,
  busy,
  playerName,
  onPlayFile,
  onChange,
  onLink,
  onOpen,
  onPlay,
}) => {
  const [open, setOpen] = useState(false);
  const media = match?.media ?? null;
  const isLinked = media !== null && linked.includes(media.id);
  const linkedElsewhere = linked.length > 0 && !isLinked;
  const episodeCount = episodeFiles(series).length;
  const firstEpisode = series.episodes[0];

  const localDetails = [
    series.episodes.length
      ? series.episodes.length === 1
        ? t('common.episodeNumber', { number: episodeRanges(series.episodes) })
        : t('localFilesTab.episodesRange', { range: episodeRanges(series.episodes) })
      : null,
    t('localFilesTab.fileCount', { count: series.files.length }),
    series.extras ? t('localFilesTab.extraCount', { count: series.extras }) : null,
    series.release_groups.slice(0, 2).join(', ') || null,
    series.resolutions.slice(0, 2).join(', ') || null,
    formatBytes(series.total_size) || null,
  ].filter(Boolean);

  const parsedLabel = [
    series.title || t('localFilesTab.noTitle'),
    series.season ? t('localFilesTab.season', { season: series.season }) : null,
    series.part ? t('localFilesTab.part', { part: series.part }) : null,
    KIND_LABELS[series.kind] ? t(KIND_LABELS[series.kind]) : null,
    series.year ? String(series.year) : null,
  ]
    .filter(Boolean)
    .join(' · ');

  let badge: React.ReactNode = null;
  if (!series.matchable) {
    badge = <span className="text-xs text-text-muted">{t('localFilesTab.notMatchable')}</span>;
  } else if (isLinked) {
    badge = (
      <span className="flex items-center gap-1 text-xs text-state-success">
        <Check className="h-3.5 w-3.5" aria-hidden="true" />
        {t('localFilesTab.linked')}
      </span>
    );
  } else if (!match) {
    badge = <span className="text-xs text-text-muted">{t('localFilesTab.waiting')}</span>;
  } else if (match.status === 'confirmed') {
    badge = <span className="text-xs text-accent-300">{t('localFilesTab.confirmed')}</span>;
  } else if (match.status === 'auto' && media) {
    badge = (
      <span className="text-xs text-state-warning tabular-nums">
        {t('localFilesTab.suggested', { percent: Math.round(media.score * 100) })}
      </span>
    );
  } else {
    badge = <span className="text-xs text-state-warning">{t('localFilesTab.noMatch')}</span>;
  }

  return (
    <li className="flex flex-col rounded-md ui-card bg-bg-card [content-visibility:auto] [contain-intrinsic-size:auto_132px]">
      <div className="flex gap-3 p-3">
        <CoverThumb src={media?.cover_image} className="h-[108px] w-[76px] flex-shrink-0 rounded-sm" />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
            <span
              className="truncate text-[14px] font-semibold text-text-primary select-text"
              title={media ? candidateTitle(media) : parsedLabel}
            >
              {media ? candidateTitle(media) : series.title || t('localFilesTab.unrecognised')}
            </span>
            {badge}
          </div>
          {media && <span className="text-xs text-text-secondary">{candidateDetails(media)}</span>}
          <span className="truncate text-xs text-text-muted select-text" title={parsedLabel}>
            {t('localFilesTab.fromFiles', { label: parsedLabel })}
          </span>
          <span className="text-xs text-text-muted tabular-nums">{localDetails.join(' · ')}</span>
          {linkedElsewhere && (
            <span className="text-xs text-text-muted">{t('localFilesTab.linkedElsewhere')}</span>
          )}

          <div className="mt-auto flex flex-wrap items-center gap-2 pt-1">
            {media && !isLinked && episodeCount > 0 && (
              <Button size="sm" variant="primary" disabled={busy} onClick={() => onLink(media)}>
                <Link2 className="h-3.5 w-3.5" aria-hidden="true" />
                <span>{t('localFilesTab.linkToTitle')}</span>
              </Button>
            )}
            {media && isLinked && firstEpisode !== undefined && (
              <Button size="sm" variant="primary" onClick={() => onPlay(media.id, firstEpisode)}>
                <Play className="h-3.5 w-3.5 fill-current" aria-hidden="true" />
                <span>{t('localFilesTab.playEpisode', { episode: episodeLabel(firstEpisode) })}</span>
              </Button>
            )}
            {media && (
              <Button size="sm" variant="neutral" onClick={() => onOpen(media.id)}>
                <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
                <span>{t('localFilesTab.openTitle')}</span>
              </Button>
            )}
            {series.matchable && (
              <Button size="sm" variant="ghost" disabled={busy} onClick={onChange}>
                {media ? t('localFilesTab.changeMatch') : t('localFilesTab.findTitle')}
              </Button>
            )}
            {busy && <Spinner size="sm" label={t('localFilesTab.working')} className="text-text-muted" />}
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setOpen((v) => !v)}
              aria-expanded={open}
              className="ms-auto"
            >
              <span>{t('localFilesTab.files')}</span>
              <ChevronDown className={clsx('h-3.5 w-3.5 transition-transform', open && 'rotate-180')} aria-hidden="true" />
            </Button>
          </div>
        </div>
      </div>

      {open && (
        <ul className="flex flex-col border-t border-ink/10 px-3 py-2">
          {series.files.map((file) => {
            const p = file.parsed;
            const what =
              p.kind === 'extra'
                ? t('localFilesTab.extra')
                : p.episode !== null
                  ? p.episode_end !== null
                    ? t('localFilesTab.episodesFromTo', { from: p.episode, to: p.episode_end })
                    : episodeLabel(p.episode)
                  : t('localFilesTab.noEpisodeNumber');
            return (
              <li key={file.file_path} className="flex items-center gap-3 py-1.5">
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-xs text-text-primary select-text" title={file.file_path}>
                    {file.file_name}
                  </span>
                  <span className="text-[11px] text-text-muted tabular-nums">
                    {[what, p.release_group, p.resolution, p.version ? `v${p.version}` : null, formatBytes(file.file_size) || null]
                      .filter(Boolean)
                      .join(' · ')}
                  </span>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => onPlayFile(file)}
                  aria-label={t('localFilesTab.playFileIn', { file: file.file_name, player: playerName })}
                  className="flex-shrink-0"
                >
                  <Play className="w-3.5 h-3.5 fill-current" aria-hidden="true" />
                  <span>{playerName}</span>
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </li>
  );
};
