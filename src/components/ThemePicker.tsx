import React, { useRef } from 'react';
import { Check } from 'lucide-react';
import clsx from 'clsx';
import { THEMES } from '../theme/themes';
import type { ThemeId } from '../theme/themes';
import { t } from '../i18n';

interface ThemePickerProps {
  value: ThemeId;
  onChange: (theme: ThemeId) => void;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
}

/**
 * A miniature of the app in one theme. `data-theme-scope` gives the element that theme's
 * tokens, so the preview is drawn with the same classes as the real thing.
 */
const ThemePreview: React.FC<{ theme: ThemeId }> = ({ theme }) => (
  <span
    aria-hidden="true"
    data-theme-scope={theme}
    className="flex h-[68px] w-full overflow-hidden rounded-md border border-border-strong bg-bg-page"
  >
    <span className="flex h-full w-4 flex-shrink-0 flex-col items-center gap-1 border-e border-border bg-bg-deepest pt-2">
      <span className="h-1 w-2 rounded-full bg-accent-500" />
      <span className="h-1 w-2 rounded-full bg-ink/25" />
      <span className="h-1 w-2 rounded-full bg-ink/25" />
    </span>
    <span className="flex min-w-0 flex-1 flex-col justify-between p-1.5">
      <span className="flex items-center justify-between gap-1 rounded-sm border border-border bg-bg-card px-1.5 py-1">
        <span className="text-[13px] font-semibold leading-none text-text-primary">Aa</span>{/* i18n-ignore */}
        <span className="h-1 flex-1 rounded-full bg-[var(--text-muted)]" />
        <span className="h-1.5 w-1.5 rounded-full bg-state-success" />
      </span>
      <span className="flex items-center gap-1">
        <span className="flex h-3.5 flex-1 items-center justify-center rounded-sm bg-accent-600">
          <span className="h-0.5 w-4 rounded-full bg-on-accent" />
        </span>
        <span className="h-3.5 flex-1 rounded-sm border border-border-strong bg-bg-elevated" />
      </span>
    </span>
  </span>
);

/** One card per built-in theme. Behaves as a radio group: arrows move and select. */
export const ThemePicker: React.FC<ThemePickerProps> = ({ value, onChange, ...aria }) => {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const handleKeyDown = (e: React.KeyboardEvent, index: number) => {
    // Right to left, the next item is drawn on the left: swap the two horizontal arrows.
    const rtl = getComputedStyle(e.currentTarget).direction === 'rtl';
    const key = rtl && e.key === 'ArrowLeft' ? 'ArrowRight' : rtl && e.key === 'ArrowRight' ? 'ArrowLeft' : e.key;
    let next: number;
    if (key === 'ArrowRight' || key === 'ArrowDown') next = (index + 1) % THEMES.length;
    else if (key === 'ArrowLeft' || key === 'ArrowUp') next = (index - 1 + THEMES.length) % THEMES.length;
    else if (key === 'Home') next = 0;
    else if (key === 'End') next = THEMES.length - 1;
    else return;
    e.preventDefault();
    onChange(THEMES[next].id);
    refs.current[next]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-labelledby={aria['aria-labelledby']}
      aria-describedby={aria['aria-describedby']}
      className="grid w-full grid-cols-2 gap-2 sm:grid-cols-3"
    >
      {THEMES.map((theme, index) => {
        const selected = theme.id === value;
        return (
          <button
            key={theme.id}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(theme.id)}
            onKeyDown={(e) => handleKeyDown(e, index)}
            className={clsx(
              'ui-theme-card flex min-w-0 flex-col gap-2 rounded-lg border p-1.5 text-start cursor-pointer transition-colors',
              selected
                ? 'border-accent-500 bg-accent-tint'
                : 'border-border bg-bg-surface hover:border-border-strong hover:bg-bg-input'
            )}
          >
            <ThemePreview theme={theme.id} />
            <span className="flex w-full items-center justify-between gap-1 px-1 pb-0.5">
              <span
                className={clsx(
                  'truncate text-xs font-medium',
                  selected ? 'text-accent-300' : 'text-text-secondary'
                )}
              >
                {'nameKey' in theme ? t(theme.nameKey) : theme.name}
              </span>
              {selected && <Check aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0 text-accent-300" />}
            </span>
          </button>
        );
      })}
    </div>
  );
};
