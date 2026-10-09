import React, { useId, useRef, useState } from 'react';
import { Button, Modal, errorMessage } from '../ui';
import { CustomSelect } from '../CustomSelect';
import { useSaveAniListEntry } from '../../hooks/useAniList';
import { useAccountStatus } from '../../hooks/useAccounts';
import { useUIStore } from '../../stores/uiStore';
import type { MediaListEntry } from '../../types';
import { ANILIST_LIST_STATUSES, INPUT_CLASS, LABEL_CLASS } from './format';

interface EditListEntryModalProps {
  entry: MediaListEntry;
  onClose: () => void;
}

function clampProgress(value: number, total: number | null): number {
  if (!Number.isFinite(value)) return 0;
  const whole = Math.max(0, Math.floor(value));
  return total !== null ? Math.min(whole, total) : whole;
}

/** Status and progress of one AniList entry. Mount it to open it. */
export const EditListEntryModal: React.FC<EditListEntryModalProps> = ({ entry, onClose }) => {
  const formId = useId();
  const saveEntry = useSaveAniListEntry();
  const showToast = useUIStore((s) => s.showToast);
  const { data: accounts } = useAccountStatus();

  const total = entry.total_episodes !== null && entry.total_episodes > 0 ? entry.total_episodes : null;
  const [status, setStatus] = useState(entry.status ?? ANILIST_LIST_STATUSES[0].value);
  const [progress, setProgress] = useState(String(clampProgress(entry.progress, total)));
  const [error, setError] = useState<string | null>(null);

  const pending = saveEntry.isPending;
  // `isPending` only changes on the next render, so a ref is what stops a fast double submit.
  const submittingRef = useRef(false);

  const handleClose = () => {
    if (!pending) onClose();
  };

  const handleStatusChange = (next: string) => {
    setStatus(next);
    // A completed anime has every episode watched.
    if (next === 'COMPLETED' && total !== null) setProgress(String(total));
  };

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (pending || submittingRef.current) return;
    submittingRef.current = true;
    setError(null);
    try {
      const saved = await saveEntry.mutateAsync({
        mediaId: entry.id,
        status,
        progress: clampProgress(Number(progress), total),
      });
      if (saved.mal.state === 'failed') {
        showToast({
          type: 'warning',
          title: 'Saved to AniList, but not to MyAnimeList',
          message: saved.mal.message,
        });
      } else {
        showToast({
          type: 'success',
          title: saved.mal.state === 'saved' ? 'Saved to AniList and MyAnimeList' : 'Saved to AniList',
        });
      }
      onClose();
    } catch (err) {
      setError(errorMessage(err, 'The entry could not be saved.'));
    } finally {
      submittingRef.current = false;
    }
  };

  return (
    <Modal
      isOpen
      onClose={handleClose}
      title={entry.title ?? 'Edit list entry'}
      size="sm"
      header={
        <p className="mt-0.5 text-xs text-text-muted">
          {accounts?.mal.connected ? 'Saved to AniList and MyAnimeList.' : 'Saved to AniList.'}
        </p>
      }
      footer={
        <>
          <Button variant="ghost" onClick={handleClose} disabled={pending}>
            Cancel
          </Button>
          <Button type="submit" form={formId} variant="primary" disabled={pending}>
            {pending ? 'Saving…' : 'Save'}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={handleSubmit} className="flex flex-col gap-4">
        <div>
          <span id={`${formId}-status`} className={LABEL_CLASS}>
            Status
          </span>
          <CustomSelect
            aria-labelledby={`${formId}-status`}
            className="w-full"
            value={status}
            onChange={handleStatusChange}
            options={ANILIST_LIST_STATUSES}
            disabled={pending}
          />
        </div>

        <div>
          <label htmlFor={`${formId}-progress`} className={LABEL_CLASS}>
            Episodes watched
          </label>
          <div className="flex items-center gap-2">
            <input
              id={`${formId}-progress`}
              data-autofocus
              type="number"
              inputMode="numeric"
              min={0}
              max={total ?? undefined}
              step={1}
              value={progress}
              onChange={(event) => setProgress(event.target.value)}
              onBlur={() => setProgress(String(clampProgress(Number(progress), total)))}
              disabled={pending}
              className={`${INPUT_CLASS} w-24 tabular-nums`}
            />
            <span className="text-[13px] text-text-muted tabular-nums">of {total ?? '?'}</span>
          </div>
        </div>

        {error && (
          <p role="alert" className="text-xs text-state-error select-text">
            {error}
          </p>
        )}
      </form>
    </Modal>
  );
};
