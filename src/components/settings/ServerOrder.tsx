import React, { useRef, useState } from 'react';
import { ChevronDown, ChevronUp, GripVertical } from 'lucide-react';
import { Row } from './SettingsLayout';
import { Button } from '../ui/Button';
import { Switch } from '../ui/Switch';
import { errorMessage } from '../ui/ErrorState';
import { usePlugins, useSetPluginEnabled } from '../../hooks/usePlugins';
import { audioOnlyLabel } from '../../lib/languages';
import { useUIStore } from '../../stores/uiStore';
import { t } from '../../i18n';
import type { PluginInfo } from '../../types';

/** Plugin ids in the saved order; plugins it does not name follow in their own order. */
export function orderPlugins(ids: string[], saved: string[] | undefined): string[] {
  const known = (saved ?? []).filter((id, i, all) => ids.includes(id) && all.indexOf(id) === i);
  return [...known, ...ids.filter((id) => !known.includes(id))];
}

/** `order` with `id` moved to `to` (clamped to the list). */
export function moveTo(order: string[], id: string, to: number): string[] {
  const from = order.indexOf(id);
  if (from < 0) return order;
  const next = order.filter((other) => other !== id);
  next.splice(Math.max(0, Math.min(to, next.length)), 0, id);
  return next;
}

/**
 * Settings › Sources: the order Auto tries the plugins in, by dragging or with the arrow
 * buttons, with each plugin's switch. The first plugin that is on is tried first.
 */
export const ServerOrder: React.FC<{ value: string[] | undefined; onChange: (order: string[]) => void }> = ({
  value,
  onChange,
}) => {
  const plugins = usePlugins().data ?? [];
  const setEnabled = useSetPluginEnabled();
  const showToast = useUIStore((s) => s.showToast);
  const listRef = useRef<HTMLOListElement>(null);
  const [dragged, setDragged] = useState<string | null>(null);
  const [over, setOver] = useState<number | null>(null);

  const byId = new Map(plugins.map((plugin) => [plugin.id, plugin]));
  const order = orderPlugins(plugins.map((plugin) => plugin.id), value);
  const firstOn = order.find((id) => byId.get(id)?.enabled && byId.get(id)?.status === 'ready');
  if (plugins.length < 2) return null;

  const move = (id: string, step: -1 | 1) => {
    onChange(moveTo(order, id, order.indexOf(id) + step));
    // The button moved with its row; keep focus on the same button of that row.
    requestAnimationFrame(() =>
      listRef.current?.querySelector<HTMLButtonElement>(`[data-row="${id}"][data-step="${step}"]:not(:disabled)`)?.focus()
    );
  };

  const toggle = (plugin: PluginInfo, enabled: boolean) =>
    setEnabled.mutate(
      { id: plugin.id, enabled },
      {
        onError: (err) =>
          showToast({
            type: 'error',
            title: t(enabled ? 'serverOrder.couldNotTurnOn' : 'serverOrder.couldNotTurnOff', { name: plugin.name.trim() || plugin.id }),
            message: errorMessage(err),
          }),
      }
    );

  const drop = (to: number) => {
    if (dragged) onChange(moveTo(order, dragged, to > order.indexOf(dragged) ? to - 1 : to));
    setDragged(null);
    setOver(null);
  };

  return (
    <Row
      label={t('serverOrder.label')}
      hint={t('serverOrder.hint')}
      keywords={t('serverOrder.keywords')}
      stacked
    >
      {({ labelId, hintId }) => (
        <ol
          ref={listRef}
          aria-labelledby={labelId}
          aria-describedby={hintId}
          className="flex max-w-xl flex-col gap-1"
          onDragOver={(e) => dragged && e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            drop(over ?? order.length);
          }}
        >
          {order.map((id, i) => {
            const plugin = byId.get(id);
            if (!plugin) return null;
            const name = plugin.name.trim() || plugin.id;
            const only = audioOnlyLabel(plugin);
            return (
              <li
                key={id}
                draggable
                onDragStart={(e) => {
                  setDragged(id);
                  e.dataTransfer.effectAllowed = 'move';
                  e.dataTransfer.setData('text/plain', id);
                }}
                onDragEnd={() => {
                  setDragged(null);
                  setOver(null);
                }}
                onDragOver={(e) => {
                  if (!dragged) return;
                  e.preventDefault();
                  const box = e.currentTarget.getBoundingClientRect();
                  setOver(e.clientY < box.top + box.height / 2 ? i : i + 1);
                }}
                className={`flex h-11 items-center gap-2 rounded-md bg-bg-elevated ps-1 pe-1 ${
                  dragged === id ? 'opacity-50' : ''
                } ${dragged && over !== null && Math.min(over, order.length - 1) === i ? 'ring-2 ring-accent-400' : ''}`}
              >
                <GripVertical aria-hidden="true" className="h-4 w-4 flex-shrink-0 cursor-grab text-text-muted" />
                <span className="w-5 text-xs tabular-nums text-text-muted">{i + 1}</span>
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-[13px] text-text-primary">{name}</span>
                  {(only || id === firstOn) && (
                    <span className="truncate text-[11px] text-text-muted">
                      {[id === firstOn ? t('serverOrder.triedFirst') : null, only].filter(Boolean).join(' · ')}
                    </span>
                  )}
                </span>
                <Button
                  icon
                  variant="ghost"
                  size="sm"
                  data-row={id}
                  data-step={-1}
                  aria-label={t('serverOrder.moveUp', { name })}
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
                  aria-label={t('serverOrder.moveDown', { name })}
                  disabled={i === order.length - 1}
                  onClick={() => move(id, 1)}
                >
                  <ChevronDown aria-hidden="true" className="h-4 w-4" />
                </Button>
                <Switch aria-label={t('serverOrder.use', { name })} checked={plugin.enabled} onChange={(on) => toggle(plugin, on)} />
              </li>
            );
          })}
        </ol>
      )}
    </Row>
  );
};
