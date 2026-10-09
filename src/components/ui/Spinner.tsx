import React from 'react';
import { Loader2 } from 'lucide-react';
import clsx from 'clsx';

interface SpinnerProps {
  size?: 'sm' | 'md' | 'lg';
  /** Announced to screen readers. */
  label?: string;
  className?: string;
}

const SIZE_CLASSES = {
  sm: 'w-3.5 h-3.5',
  md: 'w-4 h-4',
  lg: 'w-6 h-6',
};

export const Spinner: React.FC<SpinnerProps> = ({ size = 'md', label = 'Loading', className }) => (
  <span role="status" className="inline-flex items-center">
    <Loader2 aria-hidden="true" className={clsx('animate-spin', SIZE_CLASSES[size], className)} />
    <span className="sr-only">{label}</span>
  </span>
);
