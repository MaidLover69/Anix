import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Bell, CheckCheck, DownloadCloud, Radio, Trash2 } from 'lucide-react';
import clsx from 'clsx';
import { Button, EmptyState, ErrorState, SegmentedControl, Spinner, errorMessage } from '../components/ui';
import type { SegmentOption } from '../components/ui';
import {
  useClearNotices,
  useMarkNoticesRead,
  useNotices,
  usePushTestNotice,
  useUnreadNoticeCount,
} from '../hooks/useNotices';
import { useUIStore } from '../stores/uiStore';
import type { Notice } from '../types';
import { splitByDay } from '../lib/time';
import { locale, t } from '../i18n';
import { Meme, MemeImage, useHighModule } from '../components/meme/Meme';

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
  // Aired, but no extension has it yet.
  airing_missing: <Radio aria-hidden="true" className="h-4 w-4" />,
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

  const markOne = (notice: Notice) =>
    markRead.mutate({ ids: [notice.id] }, { onError: fail(t('notifications.couldNotMarkRead')) });

  const open = (notice: Notice) => {
    if (!notice.read) markOne(notice);
    const route = noticeRoute(notice.link);
    if (route) navigate(route);
  };

  const tabs: SegmentOption<NoticeTab>[] = [
    { value: 'all', label: t('notifications.all') },
    { value: 'airing', label: t('notifications.airing') },
    { value: 'download', label: t('notifications.downloads') },
  ];
  const list = notices.data ?? [];
  const groups = splitByDay(list);
  const high = useHighModule();
  // High: new-episode rows take the pool's lines in turn, so neighbours never repeat one.
  const airingTurn = new Map(list.filter((n) => n.kind === 'airing').map((n, i) => [n.id, i]));

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-col gap-0.5">
          <h1 className="meme-title text-xl font-semibold text-text-primary">
            <Meme spot="notTitle" plain={t('notifications.title')} />
          </h1>
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

      <SegmentedControl aria-label={t('notifications.types')} options={tabs} value={tab} onChange={setTab} className="self-start" />

      <div>
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
            mood="sleepy"
            meme="notEmpty"
            action={
              <Button
                variant="secondary"
                onClick={() => pushTest.mutate(undefined, { onError: fail(t('notifications.couldNotAddTest')) })}
                disabled={pushTest.isPending}
              >
                <Bell aria-hidden="true" className="h-4 w-4" />
                {t('notifications.sendTest')}
              </Button>
            }
          />
        ) : (
          <div className="flex flex-col gap-5">
            {([
              ['today', groups.today],
              ['earlier', groups.earlier],
            ] as const).map(([group, items]) =>
              items.length === 0 ? null : (
                <section key={group} aria-labelledby={`notices-${group}`} className="flex flex-col gap-2">
                  <h2 id={`notices-${group}`} className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                    {group === 'today' ? t('notifications.today') : t('notifications.earlier')}
                  </h2>
                  <ul className="flex flex-col gap-1.5">
                    {items.map((notice) => (
                      // The open button and Mark read are siblings: a button inside a button is invalid.
                      <li
                        key={notice.id}
                        data-unread={!notice.read}
                        className="ui-note flex items-center gap-1 rounded-md ui-card bg-bg-card pe-2"
                      >
                        {!notice.read && (
                          <span
                            aria-hidden="true"
                            className="absolute -start-1 top-1/2 h-2.5 w-2.5 -translate-y-1/2 rounded-full bg-accent-500 ring-[3px] ring-bg-page"
                          />
                        )}
                        <button
                          type="button"
                          onClick={() => open(notice)}
                          className="flex min-w-0 flex-1 items-start gap-3 rounded-md px-3 py-2.5 text-start cursor-pointer transition-colors hover:bg-ink/5 focus-visible:-outline-offset-2"
                        >
                          <span className="mt-0.5 flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full bg-ink/[0.06] text-text-secondary">
                            {high && notice.kind === 'airing_missing' ? (
                              <MemeImage name="surprised-pikachu" className="meme-note-img" />
                            ) : (
                              KIND_ICONS[notice.kind] ?? <Bell aria-hidden="true" className="h-4 w-4" />
                            )}
                          </span>
                          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                            <span className="flex items-center gap-2">
                              {!notice.read && <span className="sr-only">{t('notifications.unreadPrefix')} </span>}
                              {high && notice.kind === 'airing' && (
                                <span className="mm-d flex-shrink-0 text-[13px]" aria-hidden="true">
                                  {high.NEW_EP_POOL[(airingTurn.get(notice.id) ?? 0) % high.NEW_EP_POOL.length]}
                                </span>
                              )}
                              <span
                                className={clsx(
                                  'truncate text-[13px] text-text-primary',
                                  notice.read ? 'font-medium' : 'font-semibold'
                                )}
                              >
                                {notice.title}
                              </span>
                            </span>
                            {notice.body && (
                              <span className="text-xs text-text-secondary">
                                {notice.kind === 'download' ? <Meme spot="dlFail" plain={notice.body} /> : notice.body}
                              </span>
                            )}
                            <span className="text-[11px] text-text-muted">{timeAgo(notice.created_at)}</span>
                          </span>
                        </button>
                        {!notice.read && (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => markOne(notice)}
                            disabled={markRead.isPending}
                            aria-label={t('notifications.markReadFor', { title: notice.title })}
                          >
                            {t('notifications.markRead')}
                          </Button>
                        )}
                      </li>
                    ))}
                  </ul>
                </section>
              )
            )}
          </div>
        )}
      </div>
    </div>
  );
};
