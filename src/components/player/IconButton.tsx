import React from 'react';

interface IconButtonProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'aria-label'> {
  /** Accessible name; also the tooltip unless `title` is given. */
  label: string;
  /** Marks a toggle that is currently on. */
  active?: boolean;
}

/** The 36px ghost button used for every control on top of the video. */
export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ label, active = false, title, className = '', type = 'button', children, ...rest }, ref) => (
    <button
      ref={ref}
      type={type}
      aria-label={label}
      title={title ?? label}
      className={`inline-flex h-9 min-w-9 flex-shrink-0 items-center justify-center rounded-md px-2 text-[13px] font-medium tabular-nums cursor-pointer transition-colors active:bg-white/20 disabled:opacity-40 disabled:pointer-events-none ${
        active ? 'bg-white/10 text-accent-300' : 'text-white hover:bg-white/10'
      } ${className}`}
      {...rest}
    >
      {children}
    </button>
  )
);

IconButton.displayName = 'IconButton';
