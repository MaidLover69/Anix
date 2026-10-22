import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Bell, CheckCheck, DownloadCloud, Radio, Trash2 } from 'lucide-react';
import clsx from 'clsx';
import { Button, EmptyState, ErrorState, Spinner, Tabs, errorMessage } from '../components/ui';
import type { TabItem } from '../components/ui';
import {
  useClearNotices,
  useMarkNoticesRead,
  useNotices,
  usePushTestNotice,
  useUnreadNoticeCount,
} from '../hooks/useNotices';
import { useUIStore } from '../stores/uiStore';
import type { Notice } from '../types';
import { locale, t } from '../i18n';

type NoticeTab = 'all' | 'airing' | 'download';

const TAB_KINDS: Record<NoticeTab, string | null> = { all: null, airing: 'airing', download: 'download' };

/** An in-app route for a notice's link: routes as they are, anix://anime/<id> as its page. */
export function noticeRoute(link: string | null): string | null {
  if (!link) return null;
  if (link.startsWith('/')) return link;
  const match = link.match(/^(?:anix|anilili):\/\/anime\/(-?\d+)/i);
  return match ? `/detail/${match[1]}` : null;
}

function timeAgo(millis: number, now = Date.now()): string {
  const minutes = Math.round((now - millis) / 60_000);
  if (minutes < 1) return t('notifications.justNow');
  if (minutes < 60) return t('notifications.minutesAgo', { count: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t('notifications.hoursAgo', { count: hours });
  return new Date(millis).toLocaleDateString(locale(), { day: 'numeric', month: 'short', year: 'numeric' });
}

const KIND_ICONS: Record<string, React.ReactNode> = {
  airing: <Radio aria-hidden="true" className="h-4 w-4" />,
  download: <DownloadCloud aria-hidden="true" className="h-4 w-4" />,
};

export const Notifications: React.FC = () => {
  const [tab, setTab] = useState<NoticeTab>('all');
  const navigate = useNavigate();
  const showToast = useUIStore((s) => s.showToast);
  const notices = useNotices(TAB_KINDS[tab]);
  const unread = useUnreadNoticeCount().data ?? 0;
  const markRead = useMarkNoticesRead();
  const clear = useClearNotices();
  const pushTest = usePushTestNotice();

  const fail = (title: string) => (err: unknown) => showToast({ type: 'error', title, message: errorMessage(err) });

  const open = (notice: Notice) => {
    if (!notice.read) markRead.mutate({ ids: [notice.id] }, { onError: fail(t('notifications.couldNotMarkRead')) });
    const route = noticeRoute(notice.link);
    if (route) navigate(route);
  };

  const tabs: TabItem<NoticeTab>[] = [
    { value: 'all', label: t('notifications.all') },
    { value: 'airing', label: t('notifications.airing') },
    { value: 'download', label: t('notifications.downloads') },
  ];
  const list = notices.data ?? [];

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-col gap-0.5">
          <h1 className="text-xl font-semibold text-text-primary">{t('notifications.title')}</h1>
          <p className="text-xs text-text-muted" aria-live="polite">
            {unread === 0 ? t('notifications.nothingUnread') : t('notifications.unreadCount', { count: unread })}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="neutral"
            size="sm"
            onClick={() => markRead.mutate({}, { onError: fail(t('notifications.couldNotMarkAllRead')) })}
            disabled={unread === 0 || markRead.isPending}
          >
            <CheckCheck aria-hidden="true" className="h-4 w-4" />
            {t('notifications.markAllRead')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => pushTest.mutate(undefined, { onError: fail(t('notifications.couldNotAddTest')) })}
            disabled={pushTest.isPending}
          >
            <Bell aria-hidden="true" className="h-4 w-4" />
            {t('notifications.sendTest')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => clear.mutate(undefined, { onError: fail(t('notifications.couldNotClear')) })}
            disabled={list.length === 0 || clear.isPending}
          >
            <Trash2 aria-hidden="true" className="h-4 w-4" />
            {t('notifications.clearAll')}
          </Button>
        </div>
      </div>

      <Tabs aria-label={t('notifications.types')} tabs={tabs} value={tab} onChange={setTab} panelId="notices-panel" />

      <div id="notices-panel" role="tabpanel" aria-labelledby={`notices-panel-${tab}`}>
        {notices.isLoading ? (
          <div className="flex justify-center py-20">
            <Spinner size="lg" label={t('notifications.loading')} className="text-text-muted" />
          </div>
        ) : notices.isError ? (
          <ErrorState title={t('notifications.couldNotLoad')} error={notices.error} onRetry={() => notices.refetch()} />
        ) : list.length === 0 ? (
          <EmptyState
            icon={<Bell className="w-8 h-8" aria-hidden="true" />}
            title={t('notifications.emptyTitle')}
            description={t('notifications.emptyDescription')}
          />
        ) : (
          <ul className="flex flex-col gap-1">
            {list.map((notice) => (
              <li key={notice.id}>
                <button
                  type="button"
                  onClick={() => open(notice)}
                  className={clsx(
                    'flex w-full items-start gap-3 rounded-md px-3 py-2.5 text-start cursor-pointer transition-colors hover:bg-ink/5',
                    !notice.read && 'bg-accent-tint'
                  )}
                >
                  <span className="mt-0.5 flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full bg-ink/[0.06] text-text-secondary">
                    {KIND_ICONS[notice.kind] ?? <Bell aria-hidden="true" className="h-4 w-4" />}
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="flex items-center gap-2">
                      {!notice.read && <span className="sr-only">{t('notifications.unreadPrefix')} </span>}
                      <span
                        className={clsx(
                          'truncate text-[13px] text-text-primary',
                          notice.read ? 'font-medium' : 'font-semibold'
                        )}
                      >
                        {notice.title}
                      </span>
                      {!notice.read && (
                        <span aria-hidden="true" className="h-2 w-2 flex-shrink-0 rounded-full bg-accent-500" />
                      )}
                    </span>
                    {notice.body && <span className="text-xs text-text-secondary">{notice.body}</span>}
                    <span className="text-[11px] text-text-muted">{timeAgo(notice.created_at)}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
};
