import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { LibraryFile, LocalLibraryScan, SeriesMatch } from './useLocalLibrary';
import { useDownloadDirectory } from './useDownloads';
import { errorMessage } from '../components/ui/ErrorState';
import { t } from '../i18n';

export type LocalScanStatus = 'idle' | 'scanning' | 'done' | 'error';

export interface LocalScan {
  dir: string;
  setDir: (dir: string) => void;
  status: LocalScanStatus;
  /** The scanned files grouped into series; null before the first scan. */
  library: LocalLibraryScan | null;
  /** Every scanned file. */
  files: LibraryFile[];
  /** AniList matches by series key: cached ones from the scan, then newly found or chosen. */
  matches: Record<string, SeriesMatch>;
  setMatches: (found: SeriesMatch[]) => void;
  /** Titles linked from this tab since the scan, by series key. */
  linked: Record<string, number[]>;
  markLinked: (seriesKey: string, anilistId: number) => void;
  /** The folder the current `files` / `error` belong to. */
  scannedDir: string | null;
  error: string | null;
  scan: () => Promise<void>;
}

function validateDir(dir: string): string | null {
  if (!dir) return t('useLocalScan.enterFolder');
  if (!dir.startsWith('/')) {
    return t('useLocalScan.fullPath');
  }
  return null;
}

/**
 * Scan state for the Local Files tab: the files grouped into series and their AniList
 * matches. It lives above the tab so results survive tab switches; `active` says whether the
 * tab is showing. The folder field starts at the app's download folder until the user types
 * something else.
 */
export function useLocalScan({ active }: { active: boolean }): LocalScan {
  const [typedDir, setTypedDir] = useState<string | null>(null);
  const [status, setStatus] = useState<LocalScanStatus>('idle');
  const [library, setLibrary] = useState<LocalLibraryScan | null>(null);
  const [matches, setMatchState] = useState<Record<string, SeriesMatch>>({});
  const [linked, setLinked] = useState<Record<string, number[]>>({});
  const [scannedDir, setScannedDir] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const runningRef = useRef(false);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const { data: downloadDir } = useDownloadDirectory(active && typedDir === null);
  const dir = typedDir ?? downloadDir ?? '';

  const scan = useCallback(async () => {
    if (runningRef.current) return;
    const target = dir.trim();
    const invalid = validateDir(target);
    setScannedDir(target || null);
    if (invalid) {
      setLibrary(null);
      setError(invalid);
      setStatus('error');
      return;
    }

    runningRef.current = true;
    setError(null);
    setStatus('scanning');
    try {
      const found = await invoke<LocalLibraryScan>('scan_local_library', { dirPath: target });
      if (!mountedRef.current) return;
      const cached: Record<string, SeriesMatch> = {};
      for (const series of found.series) {
        if (series.series_match) cached[series.key] = series.series_match;
      }
      setMatchState(cached);
      setLinked({});
      setLibrary(found);
      setStatus('done');
    } catch (err) {
      if (!mountedRef.current) return;
      setLibrary(null);
      setError(errorMessage(err));
      setStatus('error');
    } finally {
      runningRef.current = false;
    }
  }, [dir]);

  const files = useMemo(() => library?.series.flatMap((s) => s.files) ?? [], [library]);

  const setMatches = useCallback((found: SeriesMatch[]) => {
    if (!mountedRef.current || found.length === 0) return;
    setMatchState((current) => {
      const next = { ...current };
      for (const match of found) next[match.key] = match;
      return next;
    });
  }, []);

  const markLinked = useCallback((seriesKey: string, anilistId: number) => {
    setLinked((current) => ({
      ...current,
      [seriesKey]: [...(current[seriesKey] ?? []).filter((id) => id !== anilistId), anilistId],
    }));
  }, []);

  return {
    dir,
    setDir: setTypedDir,
    status,
    library,
    files,
    matches,
    setMatches,
    linked,
    markLinked,
    scannedDir,
    error,
    scan,
  };
}
