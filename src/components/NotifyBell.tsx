import React from 'react';
import { Bell } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './ui/Button';
import { useNotificationBell, useSetNotificationBell } from '../hooks/useSchedule';

interface NotifyBellProps {
  /** AniList id of the anime. */
  mediaId: number;
  title: string;
  cover?: string | null;
  /**
   * `icon` is a bare icon button for a row, `overlay` a round one that sits on artwork,
   * `button` a labelled button for an action row.
   */
  variant?: 'icon' | 'overlay' | 'button';
  className?: string;
}

/**
 * Toggles desktop notifications for new episodes of one anime. Self-contained: it reads the
 * shared list of bells and saves the change itself.
 */
export const NotifyBell: React.FC<NotifyBellProps> = ({
  mediaId,
  title,
  cover = null,
  variant = 'icon',
  className,
}) => {
  const { data: enabled = false, isPending: isLoading } = useNotificationBell(mediaId);
  const setBell = useSetNotificationBell();

  const toggle = () => {
    if (setBell.isPending) return;
    setBell.mutate({ media_id: mediaId, enabled: !enabled, media_title: title, cover_image: cover });
  };

  // The name stays the same in both states; `aria-pressed` carries the state.
  const label = `Notify me about new episodes of ${title}`;
  const icon = <Bell aria-hidden="true" className={clsx('h-4 w-4', enabled && 'fill-current')} />;

  if (variant === 'button') {
    return (
      <Button
        variant={enabled ? 'secondary' : 'neutral'}
        aria-label={label}
        aria-pressed={enabled}
        disabled={isLoading}
        onClick={toggle}
        className={className}
      >
        {icon}
        Notify me
      </Button>
    );
  }

  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={enabled}
      title={enabled ? 'Notifications on' : 'Notify me about new episodes'}
      disabled={isLoading}
      onClick={toggle}
      className={clsx(
        'inline-flex h-8 w-8 flex-shrink-0 cursor-pointer items-center justify-center disabled:pointer-events-none',
        variant === 'overlay'
          ? [
              // Hidden until the poster is hovered or the bell is focused; stays once it is on.
              'rounded-md bg-black/70 transition-[opacity,background-color] hover:bg-black/90',
              enabled
                ? 'text-accent-300'
                : 'text-white opacity-0 focus-visible:opacity-100 group-hover:opacity-100',
            ]
          : [
              'rounded-md transition-colors active:opacity-80 disabled:opacity-40',
              enabled
                ? 'text-accent-300 hover:bg-accent-500/20 hover:text-accent-200'
                : 'text-text-muted hover:bg-ink/10 hover:text-text-primary',
            ],
        className
      )}
    >
      {icon}
    </button>
  );
};
