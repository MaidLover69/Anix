import React from 'react';
import { AlertCircle, RotateCcw } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './Button';

/** Tauri `invoke` rejects with a plain string, not an Error; this reads either. */
export function errorMessage(error: unknown, fallback = 'Something went wrong.'): string {
  if (typeof error === 'string') return error.trim() || fallback;
  if (error instanceof Error) return error.message || fallback;
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
  }
  return fallback;
}

interface ErrorStateProps {
  title?: string;
  /** Shown as-is. Pass `error` instead to have it read from a rejected query. */
  message?: string;
  error?: unknown;
  onRetry?: () => void;
  retryLabel?: string;
  /** Extra actions rendered next to Retry. */
  action?: React.ReactNode;
  /** Tighter padding for use inside a section rather than as a whole page. */
  compact?: boolean;
  className?: string;
}

export const ErrorState: React.FC<ErrorStateProps> = ({
  title = 'Something went wrong',
  message,
  error,
  onRetry,
  retryLabel = 'Retry',
  action,
  compact = false,
  className,
}) => {
  const text = message ?? (error !== undefined && error !== null ? errorMessage(error) : undefined);

  return (
    <div
      role="alert"
      className={clsx(
        'flex flex-col items-center justify-center px-6 text-center',
        compact ? 'py-8' : 'py-16',
        className
      )}
    >
      <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-state-error/10 text-state-error">
        <AlertCircle aria-hidden="true" className="h-6 w-6" />
      </div>
      <h3 className="text-base font-semibold text-text-primary">{title}</h3>
      {text && (
        <p className="mt-1 max-w-md break-words text-[13px] leading-5 text-text-muted select-text">{text}</p>
      )}
      {(onRetry || action) && (
        <div className="mt-4 flex items-center gap-2">
          {onRetry && (
            <Button variant="secondary" onClick={onRetry}>
              <RotateCcw aria-hidden="true" className="h-4 w-4" />
              {retryLabel}
            </Button>
          )}
          {action}
        </div>
      )}
    </div>
  );
};
