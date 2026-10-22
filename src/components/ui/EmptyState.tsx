import React from 'react';
import clsx from 'clsx';
import { usePlayful } from '../../hooks/useSettings';
import { Mascot, type MascotMood } from './Mascot';

interface EmptyStateProps {
  title: string;
  description?: React.ReactNode;
  icon?: React.ReactNode;
  /** Usually a `Button`. */
  action?: React.ReactNode;
  /** With "Playful touches" on, the mascot in this mood replaces the icon. */
  mood?: MascotMood;
  /** ASCII only (the Hacker font has no other glyphs); shown with "Playful touches" on. */
  kaomoji?: string;
  className?: string;
}

export const EmptyState: React.FC<EmptyStateProps> = ({
  title,
  description,
  icon,
  action,
  mood,
  kaomoji,
  className,
}) => {
  const playful = usePlayful();
  return (
    <div className={clsx('flex flex-col items-center justify-center px-6 py-16 text-center', className)}>
      {mood && playful ? (
        <Mascot mood={mood} className="mb-3 text-text-secondary" />
      ) : (
        icon && (
          // The tile sizes the icon, so every empty state has the same mark whatever the caller passes.
          <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-ink/[0.06] text-text-secondary [&>svg]:h-6 [&>svg]:w-6">
            {icon}
          </div>
        )
      )}
      <h3 className="text-base font-semibold text-text-primary">{title}</h3>
      {description && (
        <p className="mt-1 max-w-sm text-[13px] leading-5 text-text-muted select-text">{description}</p>
      )}
      {kaomoji && playful && (
        <span aria-hidden="true" className="mt-2 font-mono text-[13px] text-text-muted">
          {kaomoji}
        </span>
      )}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
};
