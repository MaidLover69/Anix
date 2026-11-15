import React, { useRef } from 'react';
import { ChevronDown, ChevronUp, Eye, EyeOff, X } from 'lucide-react';
import clsx from 'clsx';
import { Row } from './SettingsLayout';
import { Button } from '../ui/Button';
import { CustomSelect } from '../CustomSelect';
import { STATUS_ROW_TITLES } from '../CustomHomeRow';
import { usePlaylists } from '../../hooks/usePlaylists';
import {
  HOME_ROWS,
  STATUS_ROWS,
  addHomeRow,
  isBuiltInRow,
  livePlaylistRow,
  moveHomeRow,
  orderHomeRows,
  removeHomeRow,
  rowKind,
  toggleHiddenRow,
  type HomeRowId,
} from '../../lib/homeRows';
import { t } from '../../i18n';
import type { CustomPlaylist } from '../../types';

function rowLabel(id: HomeRowId, playlists: CustomPlaylist[] | undefined): string {
  const kind = rowKind(id);
  if (kind.kind === 'builtin') {
    const key = HOME_ROWS.find((row) => row.id === kind.id)?.labelKey;
    return key ? t(key) : id;
  }
  if (kind.kind === 'airing') return t('homeRows.airing');
  if (kind.kind === 'status') return t(STATUS_ROW_TITLES[kind.status]);
  return playlists?.find((p) => p.id === kind.playlistId)?.name ?? id;
}

/**
 * Settings › Appearance › Home rows: the order of the rows on Home (arrow buttons), which are
 * shown (eye button), and rows added from the schedule, a list status or a playlist.
 */
export const HomeRowOrder: React.FC<{
  value: string[] | undefined;
  hidden: string[] | undefined;
  onChange: (order: string[]) => void;
  onHiddenChange: (hidden: string[]) => void;
}> = ({ value, hidden, onChange, onHiddenChange }) => {
  const { data: playlists } = usePlaylists();
  const playlistIds = playlists?.map((p) => p.id);
  // A row whose playlist was deleted is left out here, and so from the order saved next.
  const order = orderHomeRows(value).filter((id) => livePlaylistRow(id, playlistIds));
  const hiddenSet = new Set(hidden ?? []);
  const listRef = useRef<HTMLOListElement>(null);

  const keepFocus = (selector: string) =>
    requestAnimationFrame(() => listRef.current?.querySelector<HTMLButtonElement>(`${selector}:not(:disabled)`)?.focus());

  const move = (id: HomeRowId, step: -1 | 1) => {
    onChange(moveHomeRow(order, id, step));
    // The button moved with its row; keep focus on the same button of that row.
    keepFocus(`[data-row="${id}"][data-step="${step}"]`);
  };

  const addOptions = [
    { value: 'airing', label: t('homeRows.airing'), group: t('homeRowOrder.groupSchedule') },
    ...STATUS_ROWS.map((status) => ({
      value: `status:${status}`,
      label: t(STATUS_ROW_TITLES[status]),
      group: t('homeRowOrder.groupLists'),
    })),
    ...(playlists ?? []).map((p) => ({ value: `playlist:${p.id}`, label: p.name, group: t('homeRowOrder.groupPlaylists') })),
  ].filter((option) => !order.includes(option.value as HomeRowId));

  return (
    <Row label={t('homeRowOrder.label')} hint={t('homeRowOrder.hint')} keywords={t('homeRowOrder.keywords')} stacked>
      {({ labelId, hintId }) => (
        <div className="flex max-w-md flex-col gap-2">
          <ol ref={listRef} aria-labelledby={labelId} aria-describedby={hintId} className="flex flex-col gap-1">
            {order.map((id, i) => {
              const label = rowLabel(id, playlists);
              const off = hiddenSet.has(id);
              return (
                <li key={id} className="flex h-10 items-center gap-2 rounded-md bg-bg-elevated ps-3 pe-1">
                  <span className="w-5 text-xs tabular-nums text-text-muted">{i + 1}</span>
                  <span className={clsx('min-w-0 flex-1 truncate text-[13px]', off ? 'text-text-muted line-through' : 'text-text-primary')}>
                    {label}
                  </span>
                  {off && <span className="text-xs text-text-muted">{t('homeRowOrder.hiddenTag')}</span>}
                  <Button
                    icon
                    variant="ghost"
                    size="sm"
                    data-row={id}
                    data-step="eye"
                    aria-label={off ? t('homeRowOrder.show', { name: label }) : t('homeRowOrder.hide', { name: label })}
                    onClick={() => {
                      onHiddenChange(toggleHiddenRow(hidden, id));
                      keepFocus(`[data-row="${id}"][data-step="eye"]`);
                    }}
                  >
                    {off ? <EyeOff aria-hidden="true" className="h-4 w-4" /> : <Eye aria-hidden="true" className="h-4 w-4" />}
                  </Button>
                  <Button
                    icon
                    variant="ghost"
                    size="sm"
                    data-row={id}
                    data-step={-1}
                    aria-label={t('homeRowOrder.moveUp', { name: label })}
                    disabled={i === 0}
                    onClick={() => move(id, -1)}
                  >
                    <ChevronUp aria-hidden="true" className="h-4 w-4" />
                  </Button>
                  <Button
                    icon
                    variant="ghost"
                    size="sm"
                    data-row={id}
                    data-step={1}
                    aria-label={t('homeRowOrder.moveDown', { name: label })}
                    disabled={i === order.length - 1}
                    onClick={() => move(id, 1)}
                  >
                    <ChevronDown aria-hidden="true" className="h-4 w-4" />
                  </Button>
                  {isBuiltInRow(id) ? (
                    <span className="w-8 flex-shrink-0" aria-hidden="true" />
                  ) : (
                    <Button
                      icon
                      variant="ghost"
                      size="sm"
                      aria-label={t('homeRowOrder.remove', { name: label })}
                      onClick={() => {
                        onChange(removeHomeRow(order, id));
                        if (off) onHiddenChange(toggleHiddenRow(hidden, id));
                        // The row and its button are gone; keep focus in the list, on the row that took its place.
                        const next = order[i + 1] ?? order[i - 1];
                        if (next) keepFocus(`[data-row="${next}"][data-step="eye"]`);
                      }}
                    >
                      <X aria-hidden="true" className="h-4 w-4" />
                    </Button>
                  )}
                </li>
              );
            })}
          </ol>
          {addOptions.length > 0 && (
            <CustomSelect
              size="sm"
              className="w-64"
              aria-label={t('homeRowOrder.add')}
              placeholder={t('homeRowOrder.add')}
              value=""
              options={addOptions}
              onChange={(id) => onChange(addHomeRow(order, id))}
            />
          )}
        </div>
      )}
    </Row>
  );
};
