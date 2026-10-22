import React from 'react';
import { useNavigate } from 'react-router-dom';
import { LIST_STATUS_OPTIONS, listStatusLabel, useSetListStatus } from '../hooks/useLibrary';
import { useUIStore } from '../stores/uiStore';
import { EpisodeActionsMenu, type MenuAction } from './EpisodeActionsMenu';
import { errorMessage } from './ui';
import { t } from '../i18n';
import type { ListStatusValue } from '../types';

export interface PosterMenuTarget {
  id: number;
  title: string;
  cover: string | null;
  format: string | null;
  averageScore: number | null;
}

interface PosterContextMenuProps {
  target: PosterMenuTarget;
  /** The watchlist status of the anime, or null when it is not on the list. */
  status: ListStatusValue | null;
  at: { x: number; y: number };
  onClose: () => void;
  returnFocusTo?: HTMLElement | null;
}

/** The link that opens this anime in Anix from anywhere. */
export function animeLink(id: number): string {
  return `anix://anime/${id}`;
}

/** The menu of a poster: open the page, file it under a list status, copy its link. */
export const PosterContextMenu: React.FC<PosterContextMenuProps> = ({ target, status, at, onClose, returnFocusTo }) => {
  const navigate = useNavigate();
  const showToast = useUIStore((s) => s.showToast);
  const setStatus = useSetListStatus();

  const choose = (next: ListStatusValue | null) => {
    setStatus.mutate(
      {
        anilistId: target.id,
        title: target.title,
        cover: target.cover,
        format: target.format,
        averageScore: target.averageScore,
        status: next,
      },
      {
        onError: (err) => showToast({ type: 'error', title: t('posterContextMenu.couldNotUpdate'), message: errorMessage(err) }),
      }
    );
  };

  const copyLink = () => {
    navigator.clipboard.writeText(animeLink(target.id)).then(
      () => showToast({ type: 'success', title: t('posterContextMenu.linkCopied'), message: animeLink(target.id) }),
      (err) => showToast({ type: 'error', title: t('posterContextMenu.couldNotCopy'), message: errorMessage(err) })
    );
  };

  const actions: MenuAction[] = [
    { label: t('common.details'), onSelect: () => navigate(`/detail/${target.id}`) },
    ...LIST_STATUS_OPTIONS.map((option) => ({
      // Plain text marks the current one, so screen readers hear it too.
      label:
        option.value === status
          ? t('posterContextMenu.currentStatus', { status: listStatusLabel(option.value) })
          : listStatusLabel(option.value),
      onSelect: () => option.value !== status && choose(option.value),
    })),
    ...(status !== null ? [{ label: t('posterContextMenu.removeFromList'), onSelect: () => choose(null) }] : []),
    { label: t('posterContextMenu.copyLink'), onSelect: copyLink },
  ];

  return (
    <EpisodeActionsMenu
      at={at}
      actions={actions}
      label={t('posterContextMenu.actions', { title: target.title })}
      onClose={onClose}
      returnFocusTo={returnFocusTo}
    />
  );
};
