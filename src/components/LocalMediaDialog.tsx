import React, { useState } from 'react';
import { FilePlus2, FolderOpen, RefreshCw, Unlink, X } from 'lucide-react';
import clsx from 'clsx';
import { Modal } from './ui/Modal';
import { Button } from './ui/Button';
import { Spinner } from './ui/Spinner';
import { errorMessage } from './ui/ErrorState';
import { formatBytes, INPUT_CLASS } from './library/format';
import { t } from '../i18n';
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
      setError(t('localMediaDialog.fullPath'));
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
      title={t('localMediaDialog.title')}
      size="xl"
      header={
        <p className="mt-1 text-xs text-text-muted">
          {t('localMediaDialog.intro', { title: animeTitle })}
        </p>
      }
      footer={
        <>
          {data?.linked && (
            <Button
              variant="danger"
              className="me-auto"
              disabled={busy}
              onClick={() => run({ kind: 'unlink' })}
            >
              <Unlink aria-hidden="true" className="h-4 w-4" />
              {t('localMediaDialog.unlinkAll')}
            </Button>
          )}
          <Button variant="primary" onClick={onClose}>
            {t('localMediaDialog.done')}
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
          <span className="text-[13px] font-medium text-text-primary">{t('localMediaDialog.folder')}</span>
          <p className="text-xs text-text-secondary select-text break-all">
            {data?.folder_path ?? t('localMediaDialog.noFolder')}
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="secondary" disabled={busy} onClick={() => pick(false)}>
              <FolderOpen aria-hidden="true" className="h-4 w-4" />
              {data?.folder_path ? t('localMediaDialog.changeFolder') : t('localMediaDialog.chooseFolder')}
            </Button>
            {data?.folder_path && (
              <Button variant="neutral" disabled={busy} onClick={() => run({ kind: 'rescan' })}>
                <RefreshCw aria-hidden="true" className="h-4 w-4" />
                {t('localMediaDialog.rescan')}
              </Button>
            )}
            <Button variant="neutral" disabled={busy} onClick={() => pick(true)}>
              <FilePlus2 aria-hidden="true" className="h-4 w-4" />
              {t('localMediaDialog.addFiles')}
            </Button>
            {busy && <Spinner size="sm" label={t('localMediaDialog.working')} className="text-text-muted" />}
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
              aria-label={t('localMediaDialog.folderPath')}
              value={typedFolder}
              onChange={(event) => setTypedFolder(event.target.value)}
              placeholder={t('localMediaDialog.folderPlaceholder')}
              spellCheck={false}
              autoComplete="off"
              disabled={busy}
              className={`${INPUT_CLASS} min-w-0 flex-1`}
            />
            <Button type="submit" variant="neutral" disabled={busy || !typedFolder.trim()}>
              {t('localMediaDialog.link')}
            </Button>
          </form>
          <p className="text-xs text-text-muted">
            {t('localMediaDialog.scanHint')}
          </p>
        </div>

        {link.isLoading ? (
          <div className="flex justify-center py-6">
            <Spinner size="lg" label={t('localMediaDialog.loading')} className="text-text-muted" />
          </div>
        ) : data && data.files.length > 0 ? (
          <div className="flex flex-col gap-2">
            <span className="text-[13px] font-medium text-text-primary">
              {t('localMediaDialog.fileCount', { count: data.files.length })} ·{' '}
              {t('localMediaDialog.playableCount', { count: data.episodes.length })}
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
                  onInvalid={() => setError(t('localMediaDialog.invalidEpisode'))}
                />
              ))}
            </ul>
          </div>
        ) : (
          data?.linked && (
            <p className="text-[13px] text-text-muted">{t('localMediaDialog.noFiles')}</p>
          )
        )}

        {existingDownloads.length > 0 && (
          <p className="text-xs text-text-muted">
            {t(existingDownloads.length === 1 ? 'localMediaDialog.downloadsOne' : 'localMediaDialog.downloadsMany', {
              list: existingDownloads.map((d) => d.episode_number).join(', '),
            })}
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
    !file.exists ? t('localMediaDialog.missing') : null,
    file.episode_number === null ? t('localMediaDialog.notUsed') : null,
    duplicate ? t('localMediaDialog.duplicate') : null,
    file.manual ? t('localMediaDialog.manual') : null,
  ].filter(Boolean);

  return (
    <li className="flex items-center gap-3 rounded-md px-2 py-1.5 hover:bg-ink/5">
      <label className="flex flex-shrink-0 items-center gap-1.5 text-xs text-text-muted">
        {t('localMediaDialog.ep')}
        <input
          type="text"
          inputMode="decimal"
          aria-label={t('localMediaDialog.episodeNumberOf', { file: file.file_name })}
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
          aria-label={t('localMediaDialog.removeFile', { file: file.file_name })}
          title={t('localMediaDialog.removeHint')}
          disabled={disabled}
          onClick={onRemove}
        >
          <X aria-hidden="true" className="h-4 w-4" />
        </Button>
      )}
    </li>
  );
};
