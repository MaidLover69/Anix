import React from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Database } from 'lucide-react';
import { useOfflineStatus } from '../hooks/useSystem';
import { savedAge } from '../lib/savedNote';
import { t, type TKey } from '../i18n';

interface SavedNoteData {
  source: string | null;
  age_s: number;
}

const AGE_KEYS: Record<'minutes' | 'hours' | 'days', TKey> = {
  minutes: 'offline.ageMinutes',
  hours: 'offline.ageHours',
  days: 'offline.ageDays',
};

/**
 * "Saved page from MyAnimeList, 3 hours ago": shown on a page while saved data stands in for
 * the catalogues. `kind` and `id` name the saved copy (an anime's details, or a collection).
 */
export const SavedNote: React.FC<{ kind: 'details' | 'collection'; id: string | number }> = ({ kind, id }) => {
  const { offline, saved_at } = useOfflineStatus();
  const note = useQuery({
    queryKey: ['savedPageNote', kind, String(id), saved_at],
    queryFn: () => invoke<SavedNoteData | null>('saved_page_note', { kind, key: String(id) }),
    enabled: offline,
  });
  if (!offline || !note.data) return null;
  const { unit, count } = savedAge(note.data.age_s);
  const age = unit === 'now' ? t('offline.ageNow') : t(AGE_KEYS[unit], { count });
  return (
    <p role="status" className="flex items-center gap-2 text-xs text-text-secondary">
      <Database aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0 text-state-warning" />
      <span>
        {note.data.source
          ? t('offline.savedPage', { source: note.data.source, age })
          : t('offline.savedPageNoSource', { age })}
      </span>
    </p>
  );
};
