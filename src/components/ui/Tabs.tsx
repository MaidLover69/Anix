import React, { useRef } from 'react';
import clsx from 'clsx';

export interface TabItem<T extends string = string> {
  value: T;
  label: React.ReactNode;
  /** Shown muted after the label. */
  count?: number;
  disabled?: boolean;
}

interface TabsProps<T extends string> {
  tabs: TabItem<T>[];
  value: T;
  onChange: (value: T) => void;
  'aria-label'?: string;
  className?: string;
  /** Id of the caller's `role="tabpanel"`; tab `value` gets the id `${panelId}-${value}`. */
  panelId?: string;
}

/** Underline tabs. The caller renders the panel for the selected value. */
export function Tabs<T extends string>({ tabs, value, onChange, className, panelId, ...rest }: TabsProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const handleKeyDown = (e: React.KeyboardEvent, index: number) => {
    // Right to left, the next item is drawn on the left: swap the two horizontal arrows.
    const rtl = getComputedStyle(e.currentTarget).direction === 'rtl';
    const key = rtl && e.key === 'ArrowLeft' ? 'ArrowRight' : rtl && e.key === 'ArrowRight' ? 'ArrowLeft' : e.key;
    if (key !== 'ArrowLeft' && key !== 'ArrowRight' && key !== 'Home' && key !== 'End') return;
    e.preventDefault();
    const enabled = tabs.map((t, i) => (t.disabled ? -1 : i)).filter((i) => i !== -1);
    if (enabled.length === 0) return;
    const pos = enabled.indexOf(index);
    let next = enabled[0];
    if (key === 'End') next = enabled[enabled.length - 1];
    else if (key === 'ArrowRight') next = enabled[(pos + 1) % enabled.length];
    else if (key === 'ArrowLeft') next = enabled[(pos - 1 + enabled.length) % enabled.length];
    onChange(tabs[next].value);
    refs.current[next]?.focus();
  };

  return (
    // The rule belongs to the wrapper and the underline to the label, so both stay exactly as
    // wide as before while each tab gains 8px of padding: a larger target, and room for the
    // focus ring.
    <div className={clsx('ui-tabs border-b border-border', className)}>
      <div
        role="tablist"
        aria-label={rest['aria-label']}
        className="-mx-2 -mb-px flex items-center gap-2 overflow-x-auto no-scrollbar"
      >
        {tabs.map((tab, index) => {
          const selected = tab.value === value;
          return (
            <button
              key={tab.value}
              ref={(el) => {
                refs.current[index] = el;
              }}
              type="button"
              role="tab"
              id={panelId ? `${panelId}-${tab.value}` : undefined}
              aria-controls={panelId && selected ? panelId : undefined}
              aria-selected={selected}
              tabIndex={selected ? 0 : -1}
              disabled={tab.disabled}
              onClick={() => onChange(tab.value)}
              onKeyDown={(e) => handleKeyDown(e, index)}
              className="ui-tab group flex h-10 flex-shrink-0 items-stretch rounded-sm px-2 text-[13px] font-medium whitespace-nowrap cursor-pointer focus-visible:-outline-offset-2 disabled:opacity-40 disabled:pointer-events-none"
            >
              <span
                className={clsx(
                  'flex items-center border-b-2 transition-colors',
                  selected
                    ? 'border-accent-500 text-text-primary'
                    : 'border-transparent text-text-muted group-hover:text-text-primary'
                )}
              >
                {tab.label}
                {tab.count !== undefined && (
                  <span className="ms-2 text-xs font-normal tabular-nums text-text-muted">{tab.count}</span>
                )}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
