import { useEffect, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { useDebouncedValue } from './useDebouncedValue';
import { localMediaKey, type LocalMediaLink } from './useLocalMedia';

export type ReleaseKind = 'episode' | 'movie' | 'ova' | 'ona' | 'special' | 'extra';

export interface ParsedRelease {
  title: string;
  episode: number | null;
  episode_end: number | null;
  season: number | null;
  part: number | null;
  year: number | null;
  kind: ReleaseKind;
  release_group: string | null;
  resolution: string | null;
  checksum: string | null;
  version: number | null;
  episode_title: string | null;
}

export interface LibraryFile {
  file_path: string;
  file_name: string;
  file_size: number;
  parsed: ParsedRelease;
  linked_anilist_id: number | null;
}

export interface MatchCandidate {
  id: number;
  id_mal: number | null;
  title: {
    romaji: string | null;
    english: string | null;
    native: string | null;
    user_preferred: string | null;
  };
  synonyms: string[];
  format: string | null;
  status: string | null;
  episodes: number | null;
  season: string | null;
  season_year: number | null;
  start_year: number | null;
  cover_image: string | null;
  color: string | null;
  banner_image: string | null;
  average_score: number | null;
  genres: string[];
  is_adult: boolean;
  /** Fit to the series, 0 to 1. */
  score: number;
}

export type MatchStatus = 'auto' | 'confirmed' | 'none';

export interface SeriesMatch {
  key: string;
  status: MatchStatus;
  media: MatchCandidate | null;
  candidates: MatchCandidate[];
  updated_at: number;
}

export interface LocalSeries {
  key: string;
  title: string;
  season: number | null;
  part: number | null;
  year: number | null;
  kind: ReleaseKind;
  files: LibraryFile[];
  episodes: number[];
  extras: number;
  release_groups: string[];
  resolutions: string[];
  total_size: number;
  folder: string | null;
  folder_exclusive: boolean;
  linked_anilist_ids: number[];
  matchable: boolean;
  series_match: SeriesMatch | null;
}

export interface LocalLibraryScan {
  root: string;
  file_count: number;
  series: LocalSeries[];
}

export interface MatchRequest {
  key: string;
  title: string;
  season: number | null;
  part: number | null;
  year: number | null;
  kind: ReleaseKind;
  max_episode: number | null;
}

interface MatchBatch {
  matches: SeriesMatch[];
  pending: string[];
  retry_after_s: number | null;
  error: string | null;
}

// Same as the backend: unconfirmed matches are searched again after 30 days, misses after 7.
const AUTO_TTL_MS = 30 * 24 * 60 * 60 * 1000;
const NONE_TTL_MS = 7 * 24 * 60 * 60 * 1000;

export function isMatchFresh(match: SeriesMatch | null | undefined, now = Date.now()): boolean {
  if (!match) return false;
  if (match.status === 'confirmed') return true;
  const age = now - match.updated_at;
  return match.status === 'auto' ? age < AUTO_TTL_MS : age < NONE_TTL_MS;
}

export function matchRequest(series: LocalSeries): MatchRequest {
  return {
    key: series.key,
    title: series.title,
    season: series.season,
    part: series.part,
    year: series.year,
    kind: series.kind,
    max_episode: series.episodes.length ? series.episodes[series.episodes.length - 1] : null,
  };
}

export function candidateTitle(candidate: MatchCandidate): string {
  return (
    candidate.title.user_preferred ??
    candidate.title.romaji ??
    candidate.title.english ??
    candidate.title.native ??
    `AniList ${candidate.id}`
  );
}

export type MatchingState =
  | { phase: 'idle' }
  | { phase: 'matching'; done: number; total: number }
  | { phase: 'waiting'; seconds: number; done: number; total: number }
  | { phase: 'error'; message: string };

function cancellableSleep(ms: number, cancelled: () => boolean): Promise<void> {
  return new Promise((resolve) => {
    const started = Date.now();
    const tick = () => {
      if (cancelled() || Date.now() - started >= ms) resolve();
      else setTimeout(tick, 250);
    };
    tick();
  });
}

/**
 * Matches every series of the scan that has no fresh match. The backend answers cached
 * matches at once and searches AniList for a few series per call, spaced out, so this loop
 * only keeps asking until nothing is pending. A rate limit pauses it for the time AniList asks.
 */
export function useSeriesMatching(
  library: LocalLibraryScan | null,
  matches: Record<string, SeriesMatch>,
  onMatches: (found: SeriesMatch[]) => void
) {
  const [state, setState] = useState<MatchingState>({ phase: 'idle' });
  const [attempt, setAttempt] = useState(0);
  const matchesRef = useRef(matches);
  matchesRef.current = matches;
  const onMatchesRef = useRef(onMatches);
  onMatchesRef.current = onMatches;

  useEffect(() => {
    if (!library) return;
    let cancelled = false;
    const isCancelled = () => cancelled;
    const now = Date.now();
    const queue = library.series.filter(
      (s) => s.matchable && !isMatchFresh(matchesRef.current[s.key], now)
    );
    if (queue.length === 0) {
      setState({ phase: 'idle' });
      return;
    }
    const byKey = new Map(queue.map((s) => [s.key, matchRequest(s)]));
    const total = queue.length;

    (async () => {
      let pending = queue.map((s) => s.key);
      setState({ phase: 'matching', done: 0, total });
      while (pending.length > 0 && !cancelled) {
        const requests = pending.map((key) => byKey.get(key)).filter((r): r is MatchRequest => !!r);
        let batch: MatchBatch;
        try {
          batch = await invoke<MatchBatch>('match_local_series', { requests });
        } catch (err) {
          if (!cancelled) setState({ phase: 'error', message: String(err) });
          return;
        }
        if (batch.matches.length) onMatchesRef.current(batch.matches);
        if (cancelled) return;
        const done = total - batch.pending.length;
        if (batch.error) {
          setState({ phase: 'error', message: batch.error });
          return;
        }
        if (batch.retry_after_s) {
          setState({ phase: 'waiting', seconds: batch.retry_after_s, done, total });
          await cancellableSleep(batch.retry_after_s * 1000, isCancelled);
        }
        // Stop when a call made no progress, so a search that keeps returning the same keys
        // cannot spin.
        if (!batch.retry_after_s && batch.pending.length >= pending.length) break;
        pending = batch.pending;
        if (!cancelled) setState({ phase: 'matching', done, total });
      }
      if (!cancelled) setState({ phase: 'idle' });
    })();

    return () => {
      cancelled = true;
    };
    // `attempt` restarts the loop after an error.
  }, [library, attempt]);

  return { state, retry: () => setAttempt((n) => n + 1) };
}

/** AniList search for picking another match; debounced and cached. */
export function useMatchSearch(query: string, request: MatchRequest | null) {
  const debounced = useDebouncedValue(query.trim(), 500);
  const result = useQuery<MatchCandidate[]>({
    queryKey: ['localMatchSearch', debounced, request?.key ?? null],
    queryFn: () => invoke<MatchCandidate[]>('search_local_match', { query: debounced, request }),
    enabled: debounced.length >= 2,
    staleTime: 10 * 60 * 1000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  return { ...result, debouncedQuery: debounced, typing: debounced !== query.trim() };
}

export function confirmSeriesMatch(seriesKey: string, candidate: MatchCandidate): Promise<SeriesMatch> {
  return invoke<SeriesMatch>('set_local_series_match', { seriesKey, candidate });
}

/** Video files of the series that play as episodes (extras are left out). */
export function episodeFiles(series: LocalSeries): LibraryFile[] {
  return series.files.filter((f) => f.parsed.kind !== 'extra');
}

/**
 * Links the series' files to `anilistId` under "Your files". A folder that holds only this
 * series is linked as a folder, so later episodes are picked up by a re-scan; otherwise, or
 * when the title already has another folder, the files are added one by one.
 */
export function useLinkSeries() {
  const queryClient = useQueryClient();
  return async (series: LocalSeries, anilistId: number): Promise<LocalMediaLink> => {
    const current = await invoke<LocalMediaLink>('get_local_media', { anilistId });
    const folder = series.folder_exclusive ? series.folder : null;
    const canLinkFolder = folder !== null && (!current.folder_path || current.folder_path === folder);
    const link = canLinkFolder
      ? await invoke<LocalMediaLink>('link_local_folder', { anilistId, folderPath: folder })
      : await invoke<LocalMediaLink>('add_local_files', {
          anilistId,
          filePaths: episodeFiles(series).map((f) => f.file_path),
        });
    queryClient.setQueryData(localMediaKey(anilistId), link);
    queryClient.invalidateQueries({ queryKey: ['localEpisodeSource', anilistId] });
    return link;
  };
}
