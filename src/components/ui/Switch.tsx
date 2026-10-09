import React from 'react';
import clsx from 'clsx';

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  id?: string;
  'aria-label'?: string;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
  className?: string;
}

export const Switch: React.FC<SwitchProps> = ({ checked, onChange, disabled, className, ...rest }) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    disabled={disabled}
    onClick={() => onChange(!checked)}
    className={clsx(
      'relative inline-flex h-6 w-10 flex-shrink-0 items-center rounded-full cursor-pointer transition-colors disabled:opacity-40 disabled:pointer-events-none',
      checked ? 'bg-accent-600 hover:bg-accent-700' : 'bg-white/20 hover:bg-white/30',
      className
    )}
    {...rest}
  >
    <span
      aria-hidden="true"
      className={clsx(
        'inline-block h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-150',
        checked ? 'translate-x-[18px]' : 'translate-x-0.5'
      )}
    />
  </button>
);
