import React, { useId, useRef, useState } from 'react';
import { Button, Modal, errorMessage } from '../ui';
import { useCreatePlaylist } from '../../hooks/usePlaylists';
import type { CustomPlaylist } from '../../types';
import { INPUT_CLASS, LABEL_CLASS, TEXTAREA_CLASS } from './format';
import { t, tn } from '../../i18n';

interface CreatePlaylistModalProps {
  onClose: () => void;
  onCreated: (playlist: CustomPlaylist) => void;
}

/** Mount it to open it; unmounting clears the form. */
export const CreatePlaylistModal: React.FC<CreatePlaylistModalProps> = ({ onClose, onCreated }) => {
  const formId = useId();
  const createPlaylist = useCreatePlaylist();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [error, setError] = useState<string | null>(null);

  const pending = createPlaylist.isPending;
  // `isPending` only changes on the next render, so a ref is what stops a fast double submit.
  const submittingRef = useRef(false);
  const trimmedName = name.trim();

  const handleClose = () => {
    if (!pending) onClose();
  };

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!trimmedName || pending || submittingRef.current) return;
    submittingRef.current = true;
    setError(null);
    try {
      const playlist = await createPlaylist.mutateAsync({
        name: trimmedName,
        description: description.trim() || undefined,
      });
      onCreated(playlist);
    } catch (err) {
      setError(errorMessage(err, t('createPlaylistModal.createError')));
    } finally {
      submittingRef.current = false;
    }
  };

  return (
    <Modal
      isOpen
      onClose={handleClose}
      title={t('createPlaylistModal.title')}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={handleClose} disabled={pending}>
            {t('common.cancel')}
          </Button>
          <Button type="submit" form={formId} variant="primary" disabled={!trimmedName || pending}>
            {pending ? t('createPlaylistModal.creating') : t('createPlaylistModal.create')}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={handleSubmit} className="flex flex-col gap-4">
        <div>
          <label htmlFor={`${formId}-name`} className={LABEL_CLASS}>
            {t('createPlaylistModal.name')}
          </label>
          <input
            id={`${formId}-name`}
            data-autofocus
            type="text"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('createPlaylistModal.namePlaceholder')}
            maxLength={120}
            required
            className={INPUT_CLASS}
          />
        </div>

        <div>
          <label htmlFor={`${formId}-description`} className={LABEL_CLASS}>
            {tn('createPlaylistModal.descriptionLabel', {
              optional: <span className="font-normal text-text-muted">{t('createPlaylistModal.optional')}</span>,
            })}
          </label>
          <textarea
            id={`${formId}-description`}
            rows={3}
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            maxLength={500}
            className={TEXTAREA_CLASS}
          />
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
