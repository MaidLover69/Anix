import React from 'react';
import { AlertCircle, RotateCcw } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './Button';
import { Mascot, type MascotMood } from './Mascot';
import { usePlayful } from '../../hooks/useSettings';
import { errorCode } from '../../lib/errorCodes';
import { isKey, language, t } from '../../i18n';

export { errorCode };

/** Tauri `invoke` rejects with a plain string, not an Error; this reads either. */
export function rawErrorMessage(error: unknown, fallback?: string): string {
  const otherwise = fallback ?? t('errors.fallback');
  if (typeof error === 'string') return error.trim() || otherwise;
  if (error instanceof Error) return error.message || otherwise;
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
  }
  return otherwise;
}

/**
 * A backend message in the interface language: the backend writes English, so a message whose
 * error code (`lib/errorCodes.ts`) has a translation is shown as that; others stay as written.
 */
export function localizeError(message: string): string {
  if (language() === 'en') return message;
  const key = `errors.${errorCode(message)}`;
  return isKey(key) ? t(key) : message;
}

/** The message to show for a rejected call, in the interface language where Anix knows it. */
export function errorMessage(error: unknown, fallback?: string): string {
  return localizeError(rawErrorMessage(error, fallback));
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
  /** With "Playful touches" on, the mascot in this mood replaces the red alert icon. */
  mood?: MascotMood;
  /** Tighter padding for use inside a section rather than as a whole page. */
  compact?: boolean;
  /** More technical text for the Details section, next to the code and the message. */
  details?: string;
  className?: string;
}

export const ErrorState: React.FC<ErrorStateProps> = ({
  title: titleProp,
  message,
  error,
  onRetry,
  retryLabel: retryLabelProp,
  action,
  mood,
  compact = false,
  details,
  className,
}) => {
  const playful = usePlayful();
  const title = titleProp ?? t('common.somethingWentWrong');
  const retryLabel = retryLabelProp ?? t('common.tryAgain');
  // The code and the copied details use the backend's own words, for bug reports.
  const raw = message ?? (error !== undefined && error !== null ? rawErrorMessage(error) : undefined);
  const text = raw !== undefined ? localizeError(raw) : undefined;
  const code = raw ? errorCode(raw) : null;
  const technical = [code && `Code: ${code}`, title && `Screen: ${title}`, raw && `Message: ${raw}`, details]
    .filter(Boolean)
    .join('\n');

  return (
    <div
      role="alert"
      className={clsx(
        'flex flex-col items-center justify-center px-6 text-center',
        compact ? 'py-8' : 'py-16',
        className
      )}
    >
      {mood && playful ? (
        <Mascot mood={mood} className="mb-3 text-text-secondary" />
      ) : (
        <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-state-error/10 text-state-error">
          <AlertCircle aria-hidden="true" className="h-6 w-6" />
        </div>
      )}
      <h3 className="text-base font-semibold text-text-primary">{title}</h3>
      {text && (
        <p className="mt-1 max-w-md break-words text-[13px] leading-5 text-text-muted select-text">{text}</p>
      )}
      {code && (
        <span
          title={t('errors.quoteCode')}
          className="mt-2 rounded-sm bg-ink/[0.08] px-2 py-0.5 font-mono text-[11px] font-medium tracking-wide text-text-secondary select-text"
        >
          <span className="sr-only">{t('errors.codeLabel')} </span>
          {code}
        </span>
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
      {code && (
        <details className="mt-3 w-full max-w-md text-start">
          <summary className="cursor-pointer text-center text-xs text-text-muted hover:text-text-secondary">{t('common.details')}</summary>
          <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-md bg-bg-surface p-3 text-[11px] leading-4 text-text-secondary select-text">
            {technical}
          </pre>
          <div className="mt-1.5 flex justify-end">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                navigator.clipboard?.writeText(technical).catch(() => {});
              }}
            >
              {t('errors.copyDetails')}
            </Button>
          </div>
        </details>
      )}
    </div>
  );
};
