import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { LocalAnimeFile } from '../types';
import { useDownloadDirectory } from './useDownloads';
import { errorMessage } from '../components/ui/ErrorState';

export type LocalScanStatus = 'idle' | 'scanning' | 'done' | 'error';

export interface LocalScan {
  dir: string;
  setDir: (dir: string) => void;
  status: LocalScanStatus;
  files: LocalAnimeFile[];
  /** The folder the current `files` / `error` belong to. */
  scannedDir: string | null;
  error: string | null;
  scan: () => Promise<void>;
}

function validateDir(dir: string): string | null {
  if (!dir) return 'Enter the folder to scan.';
  if (!dir.startsWith('/')) {
    return 'Use the full path, starting with "/" (for example /home/you/Videos/Anime).';
  }
  return null;
}

/**
 * Scan state for the Local Files tab. It lives above the tab so results survive tab switches;
 * `active` says whether the tab is showing. The folder field starts at the app's download
 * folder until the user types something else.
 */
export function useLocalScan({ active }: { active: boolean }): LocalScan {
  const [typedDir, setTypedDir] = useState<string | null>(null);
  const [status, setStatus] = useState<LocalScanStatus>('idle');
  const [files, setFiles] = useState<LocalAnimeFile[]>([]);
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
      setFiles([]);
      setError(invalid);
      setStatus('error');
      return;
    }

    runningRef.current = true;
    setError(null);
    setStatus('scanning');
    try {
      const found = await invoke<LocalAnimeFile[]>('scan_local_anime_directory', { dirPath: target });
      if (!mountedRef.current) return;
      setFiles(found);
      setStatus('done');
    } catch (err) {
      if (!mountedRef.current) return;
      setFiles([]);
      setError(errorMessage(err));
      setStatus('error');
    } finally {
      runningRef.current = false;
    }
  }, [dir]);

  return { dir, setDir: setTypedDir, status, files, scannedDir, error, scan };
}
