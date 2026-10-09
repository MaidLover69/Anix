import React from 'react';
import clsx from 'clsx';

interface ChipProps {
  children: React.ReactNode;
  /** Toggle state. A selected chip uses the accent tint. */
  selected?: boolean;
  /** `accent` gives a static chip the tinted look without making it a toggle. */
  tone?: 'neutral' | 'accent';
  /** With `onClick` the chip is a toggle button; without it, a static label. */
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
}

export const Chip: React.FC<ChipProps> = ({
  children,
  selected = false,
  tone = 'neutral',
  onClick,
  disabled,
  className,
}) => {
  const accent = selected || tone === 'accent';
  const classes = clsx(
    'inline-flex h-7 items-center rounded-sm px-3 text-xs whitespace-nowrap select-none',
    accent ? 'bg-accent-tint text-accent-300' : 'bg-bg-elevated text-text-secondary',
    className
  );

  if (!onClick) return <span className={classes}>{children}</span>;

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-pressed={selected}
      className={clsx(
        classes,
        'cursor-pointer transition-colors disabled:opacity-40 disabled:pointer-events-none',
        accent ? 'hover:bg-accent-500/20' : 'hover:bg-bg-input hover:text-text-primary'
      )}
    >
      {children}
    </button>
  );
};
