import React from 'react';
import { Film, FolderSearch, Play } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { Button, EmptyState, ErrorState, Spinner, errorMessage } from '../ui';
import { useSettings } from '../../hooks/useSettings';
import { useProgressiveList } from '../../hooks/useProgressiveList';
import type { LocalScan } from '../../hooks/useLocalScan';
import { useUIStore } from '../../stores/uiStore';
import type { LocalAnimeFile } from '../../types';
import { INPUT_CLASS, episodeLabel, externalPlayerFor, formatBytes, playerLabel } from './format';
import { ShowMore } from './PosterGrid';

interface LocalFilesTabProps {
  scan: LocalScan;
}

export const LocalFilesTab: React.FC<LocalFilesTabProps> = ({ scan }) => {
  const { data: settings } = useSettings();
  const showToast = useUIStore((state) => state.showToast);
  const player = externalPlayerFor(settings);
  const isScanning = scan.status === 'scanning';

  const handlePlay = async (file: LocalAnimeFile) => {
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
          Finds .mkv, .mp4, .webm and .avi files up to four folders deep. Files open in{' '}
          {playerLabel(player)}.
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
      ) : scan.status === 'idle' ? (
        <EmptyState
          icon={<FolderSearch className="w-8 h-8" aria-hidden="true" />}
          title="No folder scanned yet"
          description="Enter the folder that holds your video files and scan it to list them here."
        />
      ) : scan.files.length === 0 ? (
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
        <LocalFileList
          files={scan.files}
          scannedDir={scan.scannedDir}
          playerName={playerLabel(player)}
          onPlay={handlePlay}
        />
      )}
    </div>
  );
};

interface LocalFileListProps {
  files: LocalAnimeFile[];
  scannedDir: string | null;
  playerName: string;
  onPlay: (file: LocalAnimeFile) => void;
}

const LocalFileList: React.FC<LocalFileListProps> = ({ files, scannedDir, playerName, onPlay }) => {
  const list = useProgressiveList(files, 120, scannedDir);

  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-text-muted">
        {files.length} {files.length === 1 ? 'file' : 'files'} in{' '}
        <span className="select-text break-all">{scannedDir}</span>
      </p>

      <ul className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-2">
        {list.visible.map((file) => {
          const title = file.parsed_title || file.file_name;
          const size = formatBytes(file.file_size);
          const details = [
            file.parsed_episode !== null ? episodeLabel(file.parsed_episode) : null,
            size || null,
          ].filter(Boolean);

          return (
            <li
              key={file.file_path}
              className="flex items-center gap-3 p-3 rounded-md bg-bg-card [content-visibility:auto] [contain-intrinsic-size:auto_76px]"
            >
              <div className="flex flex-col flex-1 min-w-0 gap-0.5">
                <span className="text-[13px] font-medium text-text-primary truncate select-text" title={title}>
                  {title}
                </span>
                {details.length > 0 && (
                  <span className="text-xs text-text-muted tabular-nums">{details.join(' · ')}</span>
                )}
                <span className="text-[11px] text-text-muted truncate select-text" title={file.file_path}>
                  {file.file_path}
                </span>
              </div>

              <Button
                variant="secondary"
                size="sm"
                onClick={() => onPlay(file)}
                aria-label={`Play ${file.file_name} in ${playerName}`}
                className="flex-shrink-0"
              >
                <Play className="w-3.5 h-3.5 fill-current" aria-hidden="true" />
                <span>Play</span>
              </Button>
            </li>
          );
        })}
      </ul>

      {list.hasMore && (
        <ShowMore
          sentinelRef={list.sentinelRef}
          onClick={list.showMore}
          remaining={files.length - list.visible.length}
        />
      )}
    </div>
  );
};
