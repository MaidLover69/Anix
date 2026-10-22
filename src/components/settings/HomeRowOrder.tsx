import React, { useRef } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { Row } from './SettingsLayout';
import { Button } from '../ui/Button';
import { HOME_ROWS, moveHomeRow, orderHomeRows, type HomeRowId } from '../../lib/homeRows';
import { t } from '../../i18n';

/** Settings › Appearance: the order of the rows on Home, changed with the arrow buttons. */
export const HomeRowOrder: React.FC<{
  value: string[] | undefined;
  onChange: (order: string[]) => void;
}> = ({ value, onChange }) => {
  const order = orderHomeRows(value);
  const listRef = useRef<HTMLOListElement>(null);

  const move = (id: HomeRowId, step: -1 | 1) => {
    onChange(moveHomeRow(order, id, step));
    // The button moved with its row; keep focus on the same button of that row.
    requestAnimationFrame(() =>
      listRef.current?.querySelector<HTMLButtonElement>(`[data-row="${id}"][data-step="${step}"]:not(:disabled)`)?.focus()
    );
  };

  return (
    <Row label={t('homeRowOrder.label')} hint={t('homeRowOrder.hint')} keywords={t('homeRowOrder.keywords')} stacked>
      {({ labelId, hintId }) => (
        <ol ref={listRef} aria-labelledby={labelId} aria-describedby={hintId} className="flex max-w-md flex-col gap-1">
          {order.map((id, i) => {
            const labelKey = HOME_ROWS.find((row) => row.id === id)?.labelKey;
            const label = labelKey ? t(labelKey) : id;
            return (
              <li key={id} className="flex h-10 items-center gap-2 rounded-md bg-bg-elevated ps-3 pe-1">
                <span className="w-5 text-xs tabular-nums text-text-muted">{i + 1}</span>
                <span className="min-w-0 flex-1 truncate text-[13px] text-text-primary">{label}</span>
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
              </li>
            );
          })}
        </ol>
      )}
    </Row>
  );
};
