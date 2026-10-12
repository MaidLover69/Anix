import React from 'react';
import clsx from 'clsx';

interface EmptyStateProps {
  title: string;
  description?: React.ReactNode;
  icon?: React.ReactNode;
  /** Usually a `Button`. */
  action?: React.ReactNode;
  className?: string;
}

export const EmptyState: React.FC<EmptyStateProps> = ({
  title,
  description,
  icon,
  action,
  className,
}) => (
  <div className={clsx('flex flex-col items-center justify-center px-6 py-16 text-center', className)}>
    {icon && (
      // The tile sizes the icon, so every empty state has the same mark whatever the caller passes.
      <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-ink/[0.06] text-text-secondary [&>svg]:h-6 [&>svg]:w-6">
        {icon}
      </div>
    )}
    <h3 className="text-base font-semibold text-text-primary">{title}</h3>
    {description && (
      <p className="mt-1 max-w-sm text-[13px] leading-5 text-text-muted select-text">{description}</p>
    )}
    {action && <div className="mt-4">{action}</div>}
  </div>
);
