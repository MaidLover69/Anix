import React, { useRef } from 'react';
import clsx from 'clsx';

export interface SegmentOption<T extends string = string> {
  value: T;
  label?: React.ReactNode;
  icon?: React.ReactNode;
  disabled?: boolean;
  /** Required for an icon-only segment. */
  'aria-label'?: string;
  title?: string;
}

interface SegmentedControlProps<T extends string> {
  options: SegmentOption<T>[];
  value: T;
  onChange: (value: T) => void;
  size?: 'sm' | 'md';
  'aria-label'?: string;
  className?: string;
}

/** A single-choice toggle row (Sub/Dub, view mode). Behaves as a radio group. */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  size = 'md',
  className,
  ...rest
}: SegmentedControlProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const handleKeyDown = (e: React.KeyboardEvent, index: number) => {
    const forward = e.key === 'ArrowRight' || e.key === 'ArrowDown';
    const backward = e.key === 'ArrowLeft' || e.key === 'ArrowUp';
    if (!forward && !backward) return;
    e.preventDefault();
    const enabled = options.map((o, i) => (o.disabled ? -1 : i)).filter((i) => i !== -1);
    if (enabled.length === 0) return;
    const pos = enabled.indexOf(index);
    const next = enabled[(pos + (forward ? 1 : -1) + enabled.length) % enabled.length];
    onChange(options[next].value);
    refs.current[next]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={rest['aria-label']}
      className={clsx('inline-flex flex-shrink-0 items-center gap-0.5 rounded-md bg-bg-input p-0.5', className)}
    >
      {options.map((option, index) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option['aria-label']}
            title={option.title}
            tabIndex={selected ? 0 : -1}
            disabled={option.disabled}
            onClick={() => onChange(option.value)}
            onKeyDown={(e) => handleKeyDown(e, index)}
            className={clsx(
              'inline-flex items-center justify-center gap-2 rounded-sm text-[13px] font-medium whitespace-nowrap select-none cursor-pointer transition-colors disabled:opacity-40 disabled:pointer-events-none',
              size === 'sm' ? 'h-7' : 'h-8',
              option.label !== undefined ? 'px-3' : size === 'sm' ? 'w-7' : 'w-8',
              selected
                ? 'bg-accent-tint text-accent-300'
                : 'text-text-secondary hover:text-text-primary'
            )}
          >
            {option.icon}
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
