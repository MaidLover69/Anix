import React, { useRef, useState } from 'react';
import { Bell, BellRing } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './ui/Button';
import { EpisodeActionsMenu, type MenuAction } from './EpisodeActionsMenu';
import {
  LEAD_CHOICES,
  leadTimeLabel,
  useBellLeadTime,
  useNotificationBell,
  useSetBellLeadTime,
  useSetNotificationBell,
} from '../hooks/useSchedule';
import { t } from '../i18n';

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
 * Desktop notifications for new episodes of one anime. While off, a click turns them on;
 * while on, a click opens a menu with the reminder time (before it airs) and "Turn off".
 * Self-contained: it reads the shared list of bells and saves the change itself.
 */
export const NotifyBell: React.FC<NotifyBellProps> = ({
  mediaId,
  title,
  cover = null,
  variant = 'icon',
  className,
}) => {
  const { data: enabled = false, isPending: isLoading } = useNotificationBell(mediaId);
  const { data: lead = 0 } = useBellLeadTime(mediaId);
  const setBell = useSetNotificationBell();
  const setLead = useSetBellLeadTime();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);

  const save = (on: boolean) => {
    if (setBell.isPending) return;
    setBell.mutate({ media_id: mediaId, enabled: on, media_title: title, cover_image: cover });
  };

  const onClick = () => {
    if (!enabled) return save(true);
    const rect = buttonRef.current?.getBoundingClientRect();
    setMenuAt(rect ? { x: rect.left, y: rect.bottom + 4 } : { x: 0, y: 0 });
  };

  const actions: MenuAction[] = [
    ...LEAD_CHOICES.map((minutes) => ({
      label: leadTimeLabel(minutes),
      checked: lead === minutes,
      disabled: setLead.isPending,
      onSelect: () => {
        if (minutes !== lead) setLead.mutate({ mediaId, minutes });
      },
    })),
    { label: t('notifyBell.turnOff'), onSelect: () => save(false) },
  ];

  // Off: a toggle (`aria-pressed`). On: it opens the reminder menu.
  const label = enabled
    ? lead > 0
      ? t('notifyBell.labelOnReminder', { title, reminder: leadTimeLabel(lead) })
      : t('notifyBell.labelOn', { title })
    : t('notifyBell.notifyAbout', { title });
  const a11y = enabled
    ? { 'aria-haspopup': 'menu' as const, 'aria-expanded': menuAt !== null }
    : { 'aria-pressed': false };
  const BellIcon = enabled && lead > 0 ? BellRing : Bell;
  const icon = <BellIcon aria-hidden="true" className={clsx('h-4 w-4', enabled && 'fill-current')} />;

  const menu = menuAt && (
    <EpisodeActionsMenu
      at={menuAt}
      label={t('notifyBell.menuLabel', { title })}
      actions={actions}
      onClose={() => setMenuAt(null)}
      returnFocusTo={buttonRef.current}
    />
  );

  if (variant === 'button') {
    return (
      <>
        <Button
          ref={buttonRef}
          variant={enabled ? 'secondary' : 'neutral'}
          aria-label={label}
          {...a11y}
          disabled={isLoading}
          onClick={onClick}
          className={className}
        >
          {icon}
          {enabled ? t('notifyBell.notifying') : t('notifyBell.notifyMe')}
        </Button>
        {menu}
      </>
    );
  }

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        aria-label={label}
        {...a11y}
        title={enabled ? t('notifyBell.titleOn') : t('notifyBell.titleOff')}
        disabled={isLoading}
        onClick={onClick}
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
      {menu}
    </>
  );
};
