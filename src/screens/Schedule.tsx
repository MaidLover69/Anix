import React, { useState, useMemo, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { Calendar, List, Grid, Menu } from 'lucide-react';
import clsx from 'clsx';
import { useAiringSchedule } from '../hooks/useSchedule';
import { ShimmerImage } from '../components/ShimmerImage';
import { NotifyBell } from '../components/NotifyBell';
import { SegmentedControl } from '../components/ui/SegmentedControl';
import { EmptyState } from '../components/ui/EmptyState';
import { ErrorState } from '../components/ui/ErrorState';
import type { AiringEntry } from '../types';

type ViewMode = 'compact' | 'list' | 'grid';

const DAY_COUNT = 7;

const VIEW_OPTIONS = [
  { value: 'compact' as const, icon: <Menu aria-hidden="true" className="h-4 w-4" />, 'aria-label': 'Compact rows', title: 'Compact rows' },
  { value: 'list' as const, icon: <List aria-hidden="true" className="h-4 w-4" />, 'aria-label': 'List', title: 'List' },
  { value: 'grid' as const, icon: <Grid aria-hidden="true" className="h-4 w-4" />, 'aria-label': 'Posters', title: 'Posters' },
];

const CONTAINER_CLASS: Record<ViewMode, string> = {
  compact: 'flex flex-col',
  list: 'grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-x-4 gap-y-2',
  grid: 'grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-6',
};

/** Unix seconds of local midnight, `dayOffset` days after the day containing `nowSeconds`. */
function startOfLocalDay(nowSeconds: number, dayOffset = 0): number {
  const d = new Date(nowSeconds * 1000);
  // Calendar arithmetic rather than adding 86400s, which drifts across DST changes.
  return Math.floor(new Date(d.getFullYear(), d.getMonth(), d.getDate() + dayOffset).getTime() / 1000);
}

function formatCountdown(airingAt: number, now: number): string {
  const diff = airingAt - now;
  if (diff <= 0) {
    const hours = Math.floor(-diff / 3600);
    const minutes = Math.floor((-diff % 3600) / 60);
    if (hours >= 24) return 'Aired';
    if (hours > 0) return `Aired ${hours}h ago`;
    return `Aired ${minutes}m ago`;
  }
  const hours = Math.floor(diff / 3600);
  const minutes = Math.floor((diff % 3600) / 60);
  if (hours >= 24) return `In ${Math.floor(hours / 24)}d ${hours % 24}h`;
  if (hours > 0) return `In ${hours}h ${minutes}m`;
  return `In ${Math.max(1, minutes)}m`;
}

/** One schedule entry; the three layouts differ only in arrangement. */
const ScheduleEntry: React.FC<{ entry: AiringEntry; now: number; variant: ViewMode }> = ({
  entry,
  now,
  variant,
}) => {
  const hasAired = entry.airing_at <= now;
  const airsSoon = !hasAired && entry.airing_at - now < 3600;
  const time = new Date(entry.airing_at * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
  const countdown = formatCountdown(entry.airing_at, now);

  // The link and the bell are siblings: a button nested inside a link is invalid and
  // cannot be used on its own from the keyboard.
  const rowClass = 'group rounded-md transition-colors hover:bg-white/5';
  // Episodes that have already aired step back.
  const linkClass = clsx('rounded-md', hasAired && 'opacity-60');
  const to = `/detail/${entry.media_id}`;
  const timeNode = <span className="text-[13px] font-medium tabular-nums text-text-secondary">{time}</span>;
  const countdownNode = (
    <span className={clsx('text-xs tabular-nums', airsSoon ? 'text-accent-300' : 'text-text-muted')}>
      {countdown}
    </span>
  );
  const bell = (kind: 'icon' | 'overlay', className?: string) => (
    <NotifyBell
      mediaId={entry.media_id}
      title={entry.media_title}
      cover={entry.cover_image}
      variant={kind}
      className={className}
    />
  );

  if (variant === 'compact') {
    return (
      <div className={clsx(rowClass, 'flex h-9 items-center gap-1 pr-1')}>
        <Link
          to={to}
          draggable={false}
          className={clsx(linkClass, 'flex h-full min-w-0 flex-1 items-center gap-4 pl-3 pr-2')}
        >
          <span className="w-[4.5rem] flex-shrink-0 whitespace-nowrap">{timeNode}</span>
          <span className="min-w-0 flex-1 truncate text-[13px] text-text-primary">{entry.media_title}</span>
          <span className="flex-shrink-0 text-xs text-text-muted">Episode {entry.episode}</span>
          <span className="w-24 flex-shrink-0 text-right">{countdownNode}</span>
        </Link>
        {bell('icon')}
      </div>
    );
  }

  if (variant === 'grid') {
    return (
      <div className={clsx(rowClass, 'relative')}>
        <Link to={to} draggable={false} className={clsx(linkClass, 'flex flex-col')}>
          <div className="relative aspect-[2/3] w-full overflow-hidden rounded-xl bg-bg-card">
            <ShimmerImage
              src={entry.cover_image}
              alt=""
              className="h-full w-full transition-transform duration-150 group-hover:scale-[1.04]"
            />
            <span className="absolute left-2 top-2 rounded-sm bg-black/70 px-1.5 py-0.5 text-[11px] font-semibold leading-[14px] tabular-nums text-white">
              {time}
            </span>
          </div>
          <span className="mt-2 text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
            {entry.media_title}
          </span>
          <span className="mt-0.5 flex items-center justify-between gap-2 text-xs text-text-muted">
            <span>Episode {entry.episode}</span>
            {countdownNode}
          </span>
        </Link>
        {bell('overlay', 'absolute right-2 top-2')}
      </div>
    );
  }

  return (
    <div className={clsx(rowClass, 'flex items-center gap-1 pr-2')}>
      <Link to={to} draggable={false} className={clsx(linkClass, 'flex min-w-0 flex-1 items-center gap-3 p-2')}>
        <span className="w-[4.5rem] flex-shrink-0 whitespace-nowrap">{timeNode}</span>
        <ShimmerImage src={entry.cover_image} alt="" className="aspect-[2/3] w-12 flex-shrink-0 rounded-md" />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="text-[13px] font-medium leading-[18px] text-text-primary line-clamp-2">
            {entry.media_title}
          </span>
          <span className="mt-0.5 flex items-center gap-2 text-xs text-text-muted">
            <span>Episode {entry.episode}</span>
            <span aria-hidden="true">·</span>
            {countdownNode}
          </span>
        </span>
      </Link>
      {bell('icon')}
    </div>
  );
};

export const Schedule: React.FC = () => {
  const [selectedDayOffset, setSelectedDayOffset] = useState(0); // 0 = today
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [viewMode, setViewMode] = useState<ViewMode>('list');

  // Drives the countdowns and, because "today" is derived from it, the day rollover.
  useEffect(() => {
    const interval = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 30000);
    return () => window.clearInterval(interval);
  }, []);

  const todayStart = startOfLocalDay(now);

  const days = useMemo(
    () =>
      Array.from({ length: DAY_COUNT }, (_, i) => {
        const start = startOfLocalDay(todayStart, i);
        const date = new Date(start * 1000);
        return {
          offset: i,
          start,
          end: startOfLocalDay(todayStart, i + 1),
          weekday: date.toLocaleDateString(undefined, { weekday: 'short' }),
          longLabel: date.toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' }),
          dayOfMonth: date.getDate(),
        };
      }),
    [todayStart]
  );

  const { data: schedule = [], isLoading, isError, error, refetch, isFetching } = useAiringSchedule(
    days[0].start,
    days[DAY_COUNT - 1].end
  );

  const activeDay = days[selectedDayOffset] ?? days[0];

  const entries = useMemo(
    () =>
      schedule
        .filter((entry) => entry.airing_at >= activeDay.start && entry.airing_at < activeDay.end)
        .sort((a, b) => a.airing_at - b.airing_at),
    [schedule, activeDay.start, activeDay.end]
  );

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-semibold text-text-primary">Schedule</h1>

        <div className="flex flex-wrap items-center gap-2">
          <SegmentedControl
            aria-label="Day"
            value={String(activeDay.offset)}
            onChange={(value) => setSelectedDayOffset(Number(value))}
            options={days.map((day) => ({
              value: String(day.offset),
              'aria-label': day.offset === 0 ? `Today, ${day.longLabel}` : day.longLabel,
              label: (
                <span className="flex items-center gap-1.5">
                  {day.offset === 0 && (
                    <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-accent-500" />
                  )}
                  <span>{day.weekday}</span>
                  <span className="tabular-nums">{day.dayOfMonth}</span>
                </span>
              ),
            }))}
          />
          <SegmentedControl aria-label="Layout" value={viewMode} onChange={setViewMode} options={VIEW_OPTIONS} />
        </div>
      </div>

      {isLoading ? (
        <div aria-busy="true" className={CONTAINER_CLASS.list}>
          {Array.from({ length: 9 }).map((_, i) => (
            <div key={i} className="h-[88px] rounded-md shimmer-placeholder" />
          ))}
        </div>
      ) : isError ? (
        <ErrorState
          title="Could not load the airing schedule"
          error={error}
          onRetry={() => refetch()}
          retryLabel={isFetching ? 'Retrying…' : 'Retry'}
        />
      ) : entries.length === 0 ? (
        <EmptyState
          icon={<Calendar aria-hidden="true" className="h-8 w-8" />}
          title="Nothing scheduled"
          description={`No episodes air on ${activeDay.longLabel}.`}
        />
      ) : (
        <div className={CONTAINER_CLASS[viewMode]}>
          {entries.map((entry) => (
            <ScheduleEntry key={entry.id} entry={entry} now={now} variant={viewMode} />
          ))}
        </div>
      )}
    </div>
  );
};

export default Schedule;
