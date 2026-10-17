import React, { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  AlertCircle,
  Check,
  ChevronDown,
  ExternalLink,
  Film,
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
import { LOCAL_PROVIDER } from '../../hooks/useLocalMedia';
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

interface LocalFilesTabProps {
  scan: LocalScan;
}

type Filter = 'all' | 'review' | 'linked';

const KIND_LABELS: Record<string, string> = {
  movie: 'Movie',
  ova: 'OVA',
  ona: 'ONA',
  special: 'Specials',
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

  const handlePlay = async (file: LibraryFile) => {
    try {
      await invoke('open_file_in_player', { filePath: file.file_path, player });
    } catch (err) {
      showToast({
        type: 'error',
        title: `Could not open the file in ${playerLabel(player)}`,
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
          Folder to scan
        </label>
        <div className="flex items-center gap-2">
          <input
            id="local-scan-dir"
            type="text"
            value={scan.dir}
            onChange={(event) => scan.setDir(event.target.value)}
            disabled={isScanning}
            placeholder="/home/you/Videos/Anime"
            spellCheck={false}
            autoComplete="off"
            className={`${INPUT_CLASS} flex-1 min-w-0 max-w-xl`}
          />
          <Button type="submit" variant="primary" disabled={isScanning}>
            {isScanning ? 'Scanning…' : 'Scan folder'}
          </Button>
        </div>
        <p className="text-xs text-text-muted">
          Finds .mkv, .mp4, .webm and .avi files up to four folders deep, groups them into series and
          matches each with AniList. Link a series to play it in the built-in player and keep your
          lists in sync.
        </p>
      </form>

      {isScanning ? (
        <div className="flex flex-col items-center gap-3 py-16 text-center">
          <Spinner size="lg" label="Scanning folder" className="text-text-muted" />
          <p className="text-[13px] text-text-secondary">
            Scanning <span className="select-text break-all">{scan.scannedDir}</span>
          </p>
          <p className="text-xs text-text-muted">Large folders and network drives can take a while.</p>
        </div>
      ) : scan.status === 'error' ? (
        <ErrorState
          title="The folder could not be scanned"
          message={scan.error ?? 'Unknown error'}
          onRetry={() => scan.scan()}
        />
      ) : scan.status === 'idle' || !library ? (
        <EmptyState
          icon={<FolderSearch className="w-8 h-8" aria-hidden="true" />}
          title="No folder scanned yet"
          description="Enter the folder that holds your video files and scan it to list them here."
        />
      ) : library.file_count === 0 ? (
        <EmptyState
          icon={<Film className="w-8 h-8" aria-hidden="true" />}
          title="No video files found"
          description={
            <>
              Nothing playable was found in{' '}
              <span className="select-text break-all">{scan.scannedDir}</span>.
            </>
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
        <span className="select-text">Matching with AniList stopped: {state.message}</span>
        <Button size="sm" variant="ghost" onClick={onRetry}>
          <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
          <span>Try again</span>
        </Button>
      </div>
    );
  }
  return (
    <div role="status" className="flex items-center gap-2 text-xs text-text-muted tabular-nums">
      <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
      {state.phase === 'waiting'
        ? `AniList asked to slow down; continuing in ${state.seconds} s (${state.done} of ${state.total} matched)`
        : `Matching with AniList… ${state.done} of ${state.total}`}
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
      showToast({ type: 'error', title: 'Could not save the match', message: errorMessage(err) });
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
          title: 'AniList asked to slow down',
          message: `Try again in ${batch.retry_after_s} s.`,
        });
      } else if (batch.error) {
        showToast({ type: 'error', title: 'The search failed', message: batch.error });
      }
    } catch (err) {
      showToast({ type: 'error', title: 'The search failed', message: errorMessage(err) });
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
        title: `Linked to ${candidateTitle(candidate)}`,
        message: `${count} ${count === 1 ? 'episode plays' : 'episodes play'} from your files on the title's page.`,
      });
    } catch (err) {
      showToast({ type: 'error', title: 'Could not link the files', message: errorMessage(err) });
    } finally {
      setBusyKey(null);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs text-text-muted">
          {library.series.length} series ·{' '}
          {library.file_count} {library.file_count === 1 ? 'file' : 'files'} in{' '}
          <span className="select-text break-all">{library.root}</span>
        </p>
        <div className="flex items-center gap-1.5" role="group" aria-label="Show">
          <Chip selected={filter === 'all'} onClick={() => setFilter('all')}>
            All
          </Chip>
          <Chip selected={filter === 'review'} onClick={() => setFilter('review')}>
            To review ({counts.review})
          </Chip>
          <Chip selected={filter === 'linked'} onClick={() => setFilter('linked')}>
            Linked ({counts.linked})
          </Chip>
        </div>
      </div>

      <MatchingStatus state={matching} onRetry={onRetryMatching} />

      {visible.length === 0 ? (
        <p className="py-10 text-center text-xs text-text-muted">Nothing to show with this filter.</p>
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
      ? `${series.episodes.length === 1 ? 'Episode' : 'Episodes'} ${episodeRanges(series.episodes)}`
      : null,
    `${series.files.length} ${series.files.length === 1 ? 'file' : 'files'}`,
    series.extras ? `${series.extras} ${series.extras === 1 ? 'extra' : 'extras'}` : null,
    series.release_groups.slice(0, 2).join(', ') || null,
    series.resolutions.slice(0, 2).join(', ') || null,
    formatBytes(series.total_size) || null,
  ].filter(Boolean);

  const parsedLabel = [
    series.title || 'Files without a recognisable title',
    series.season ? `Season ${series.season}` : null,
    series.part ? `Part ${series.part}` : null,
    KIND_LABELS[series.kind] ?? null,
    series.year ? String(series.year) : null,
  ]
    .filter(Boolean)
    .join(' · ');

  let badge: React.ReactNode = null;
  if (!series.matchable) {
    badge = <span className="text-xs text-text-muted">Not matched: no title in the file names</span>;
  } else if (isLinked) {
    badge = (
      <span className="flex items-center gap-1 text-xs text-state-success">
        <Check className="h-3.5 w-3.5" aria-hidden="true" />
        Linked
      </span>
    );
  } else if (!match) {
    badge = <span className="text-xs text-text-muted">Waiting for a match…</span>;
  } else if (match.status === 'confirmed') {
    badge = <span className="text-xs text-accent-300">Match confirmed</span>;
  } else if (match.status === 'auto' && media) {
    badge = (
      <span className="text-xs text-state-warning tabular-nums">
        Suggested match · {Math.round(media.score * 100)}% fit
      </span>
    );
  } else {
    badge = <span className="text-xs text-state-warning">No good match found</span>;
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
              {media ? candidateTitle(media) : series.title || 'Unrecognised files'}
            </span>
            {badge}
          </div>
          {media && <span className="text-xs text-text-secondary">{candidateDetails(media)}</span>}
          <span className="truncate text-xs text-text-muted select-text" title={parsedLabel}>
            From files: {parsedLabel}
          </span>
          <span className="text-xs text-text-muted tabular-nums">{localDetails.join(' · ')}</span>
          {linkedElsewhere && (
            <span className="text-xs text-text-muted">Some files are linked to another title.</span>
          )}

          <div className="mt-auto flex flex-wrap items-center gap-2 pt-1">
            {media && !isLinked && episodeCount > 0 && (
              <Button size="sm" variant="primary" disabled={busy} onClick={() => onLink(media)}>
                <Link2 className="h-3.5 w-3.5" aria-hidden="true" />
                <span>Link to this title</span>
              </Button>
            )}
            {media && isLinked && firstEpisode !== undefined && (
              <Button size="sm" variant="primary" onClick={() => onPlay(media.id, firstEpisode)}>
                <Play className="h-3.5 w-3.5 fill-current" aria-hidden="true" />
                <span>Play {episodeLabel(firstEpisode)}</span>
              </Button>
            )}
            {media && (
              <Button size="sm" variant="neutral" onClick={() => onOpen(media.id)}>
                <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
                <span>Open title</span>
              </Button>
            )}
            {series.matchable && (
              <Button size="sm" variant="ghost" disabled={busy} onClick={onChange}>
                {media ? 'Change match' : 'Find the title'}
              </Button>
            )}
            {busy && <Spinner size="sm" label="Working" className="text-text-muted" />}
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setOpen((v) => !v)}
              aria-expanded={open}
              className="ml-auto"
            >
              <span>Files</span>
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
                ? 'Extra'
                : p.episode !== null
                  ? p.episode_end !== null
                    ? `Episodes ${p.episode}–${p.episode_end}`
                    : episodeLabel(p.episode)
                  : 'No episode number';
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
                  aria-label={`Play ${file.file_name} in ${playerName}`}
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
