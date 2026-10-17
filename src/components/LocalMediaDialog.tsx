import React, { useState } from 'react';
import { FilePlus2, FolderOpen, RefreshCw, Unlink, X } from 'lucide-react';
import clsx from 'clsx';
import { Modal } from './ui/Modal';
import { Button } from './ui/Button';
import { Spinner } from './ui/Spinner';
import { errorMessage } from './ui/ErrorState';
import { formatBytes, INPUT_CLASS } from './library/format';
import {
  pickLocalMedia,
  useLocalMedia,
  useLocalMediaAction,
  type LocalMediaFile,
} from '../hooks/useLocalMedia';

interface LocalMediaDialogProps {
  isOpen: boolean;
  onClose: () => void;
  anilistId: number;
  animeTitle: string;
}

function parseEpisodeInput(value: string): number | null | 'invalid' {
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return 'invalid';
  return Number(trimmed);
}

/**
 * Links a folder and/or single video files to one title ("Your files"). Every file gets the
 * episode number its name suggests; a wrong or missing number is corrected here.
 */
export const LocalMediaDialog: React.FC<LocalMediaDialogProps> = ({
  isOpen,
  onClose,
  anilistId,
  animeTitle,
}) => {
  const link = useLocalMedia(isOpen ? anilistId : null);
  const action = useLocalMediaAction(anilistId);
  const [typedFolder, setTypedFolder] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);

  const data = link.data;
  const busy = action.isPending || picking;

  const run = (input: Parameters<typeof action.mutate>[0]) => {
    setError(null);
    action.mutate(input, { onError: (err) => setError(errorMessage(err)) });
  };

  const pick = async (files: boolean) => {
    setError(null);
    setPicking(true);
    try {
      const paths = await pickLocalMedia(files);
      if (paths.length === 0) return;
      if (files) run({ kind: 'addFiles', filePaths: paths });
      else run({ kind: 'linkFolder', folderPath: paths[0] });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setPicking(false);
    }
  };

  const linkTypedFolder = () => {
    const folder = typedFolder.trim();
    if (!folder.startsWith('/')) {
      setError('Use the full path of the folder, starting with "/".');
      return;
    }
    run({ kind: 'linkFolder', folderPath: folder });
    setTypedFolder('');
  };

  // Episodes with more than one file: the one numbered by hand, else the largest, plays.
  const counts = new Map<number, number>();
  for (const file of data?.files ?? []) {
    if (file.episode_number !== null && file.exists) {
      counts.set(file.episode_number, (counts.get(file.episode_number) ?? 0) + 1);
    }
  }
  const existingDownloads = (data?.downloads ?? []).filter((d) => d.exists);

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="Your files"
      size="xl"
      header={
        <p className="mt-1 text-xs text-text-muted">
          Episodes of {animeTitle} from your own folder. They play in the built-in player and
          update your progress and connected trackers like streams do.
        </p>
      }
      footer={
        <>
          {data?.linked && (
            <Button
              variant="danger"
              className="mr-auto"
              disabled={busy}
              onClick={() => run({ kind: 'unlink' })}
            >
              <Unlink aria-hidden="true" className="h-4 w-4" />
              Unlink all
            </Button>
          )}
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {error && (
          <p role="alert" className="text-xs text-state-error select-text break-words">
            {error}
          </p>
        )}

        <div className="flex flex-col gap-2">
          <span className="text-[13px] font-medium text-text-primary">Folder</span>
          <p className="text-xs text-text-secondary select-text break-all">
            {data?.folder_path ?? 'No folder linked.'}
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="secondary" disabled={busy} onClick={() => pick(false)}>
              <FolderOpen aria-hidden="true" className="h-4 w-4" />
              {data?.folder_path ? 'Change folder…' : 'Choose folder…'}
            </Button>
            {data?.folder_path && (
              <Button variant="neutral" disabled={busy} onClick={() => run({ kind: 'rescan' })}>
                <RefreshCw aria-hidden="true" className="h-4 w-4" />
                Re-scan
              </Button>
            )}
            <Button variant="neutral" disabled={busy} onClick={() => pick(true)}>
              <FilePlus2 aria-hidden="true" className="h-4 w-4" />
              Add files…
            </Button>
            {busy && <Spinner size="sm" label="Working" className="text-text-muted" />}
          </div>
          <form
            className="flex items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              linkTypedFolder();
            }}
          >
            <input
              type="text"
              aria-label="Folder path"
              value={typedFolder}
              onChange={(event) => setTypedFolder(event.target.value)}
              placeholder="Or type the folder path, e.g. /home/you/Videos/Anime/Show"
              spellCheck={false}
              autoComplete="off"
              disabled={busy}
              className={`${INPUT_CLASS} min-w-0 flex-1`}
            />
            <Button type="submit" variant="neutral" disabled={busy || !typedFolder.trim()}>
              Link
            </Button>
          </form>
          <p className="text-xs text-text-muted">
            Finds .mkv, .mp4, .webm and .avi files up to four folders deep. Subtitle files with the
            same name (.srt, .ass, .vtt) are picked up too.
          </p>
        </div>

        {link.isLoading ? (
          <div className="flex justify-center py-6">
            <Spinner size="lg" label="Loading your files" className="text-text-muted" />
          </div>
        ) : data && data.files.length > 0 ? (
          <div className="flex flex-col gap-2">
            <span className="text-[13px] font-medium text-text-primary">
              {data.files.length} {data.files.length === 1 ? 'file' : 'files'} · {data.episodes.length}{' '}
              {data.episodes.length === 1 ? 'episode' : 'episodes'} playable
            </span>
            <ul className="-mx-2 flex max-h-80 flex-col gap-0.5 overflow-y-auto">
              {data.files.map((file) => (
                <LocalFileRow
                  key={`${file.id}:${file.episode_number ?? ''}`}
                  file={file}
                  duplicate={file.episode_number !== null && (counts.get(file.episode_number) ?? 0) > 1}
                  disabled={busy}
                  onSetEpisode={(episodeNumber) =>
                    run({ kind: 'setEpisode', fileId: file.id, episodeNumber })
                  }
                  onRemove={() => run({ kind: 'removeFile', fileId: file.id })}
                  onInvalid={() => setError('Enter an episode number such as 3 or 12.5, or leave it empty.')}
                />
              ))}
            </ul>
          </div>
        ) : (
          data?.linked && (
            <p className="text-[13px] text-text-muted">No video files were found in the linked folder.</p>
          )
        )}

        {existingDownloads.length > 0 && (
          <p className="text-xs text-text-muted">
            Also playable from your downloads:{' '}
            {existingDownloads.length === 1 ? 'episode' : 'episodes'}{' '}
            {existingDownloads.map((d) => d.episode_number).join(', ')}.
          </p>
        )}
      </div>
    </Modal>
  );
};

interface LocalFileRowProps {
  file: LocalMediaFile;
  duplicate: boolean;
  disabled: boolean;
  onSetEpisode: (episode: number | null) => void;
  onRemove: () => void;
  onInvalid: () => void;
}

const LocalFileRow: React.FC<LocalFileRowProps> = ({
  file,
  duplicate,
  disabled,
  onSetEpisode,
  onRemove,
  onInvalid,
}) => {
  const initial = file.episode_number === null ? '' : String(file.episode_number);
  const [value, setValue] = useState(initial);

  const commit = () => {
    if (value.trim() === initial) return;
    const parsed = parseEpisodeInput(value);
    if (parsed === 'invalid') {
      onInvalid();
      setValue(initial);
      return;
    }
    onSetEpisode(parsed);
  };

  const notes = [
    formatBytes(file.file_size) || null,
    !file.exists ? 'Missing on disk' : null,
    file.episode_number === null ? 'Not used' : null,
    duplicate ? 'Another file has this number' : null,
    file.manual ? 'Numbered by you' : null,
  ].filter(Boolean);

  return (
    <li className="flex items-center gap-3 rounded-md px-2 py-1.5 hover:bg-ink/5">
      <label className="flex flex-shrink-0 items-center gap-1.5 text-xs text-text-muted">
        Ep
        <input
          type="text"
          inputMode="decimal"
          aria-label={`Episode number of ${file.file_name}`}
          value={value}
          disabled={disabled}
          onChange={(event) => setValue(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') (event.target as HTMLInputElement).blur();
            if (event.key === 'Escape') {
              event.stopPropagation();
              setValue(initial);
            }
          }}
          placeholder="–"
          className={`${INPUT_CLASS} !h-8 w-16 text-center tabular-nums`}
        />
      </label>
      <span className="flex min-w-0 flex-1 flex-col">
        <span
          title={file.file_path}
          className={clsx(
            'truncate text-[13px] select-text',
            file.exists && file.episode_number !== null ? 'text-text-primary' : 'text-text-muted'
          )}
        >
          {file.file_name}
        </span>
        {notes.length > 0 && (
          <span className={clsx('text-[11px]', file.exists ? 'text-text-muted' : 'text-state-warning')}>
            {notes.join(' · ')}
          </span>
        )}
      </span>
      {!file.from_folder && (
        <Button
          icon
          variant="ghost"
          size="sm"
          aria-label={`Remove ${file.file_name}`}
          title="Remove from this title (the file stays on disk)"
          disabled={disabled}
          onClick={onRemove}
        >
          <X aria-hidden="true" className="h-4 w-4" />
        </Button>
      )}
    </li>
  );
};
