/**
 * Date and filter logic of the Schedule screen.
 */
import { t } from '../../i18n';

export interface ScheduleItem {
  media_id: number;
  airing_at: number;
  format: string | null;
}

export interface ScheduleFilters {
  /** An AniList format (`TV`, `MOVIE`, …) or '' for all. */
  format: string;
  /** Only the watchlist and belled anime. */
  myList: boolean;
  /** Only anime with a dub known to Anix. */
  dub: boolean;
}

/** Unix seconds of local midnight, `dayOffset` days after the day containing `nowSeconds`. */
export function startOfLocalDay(nowSeconds: number, dayOffset = 0): number {
  const d = new Date(nowSeconds * 1000);
  // Calendar arithmetic rather than adding 86400s, which drifts across DST changes.
  return Math.floor(new Date(d.getFullYear(), d.getMonth(), d.getDate() + dayOffset).getTime() / 1000);
}

/** "in 13h 2m", "in 3d 4h", "in 5m"; "aired 2h ago" once it has aired, "aired" after a day. */
export function formatCountdown(airingAt: number, now: number): string {
  const diff = airingAt - now;
  const span = Math.abs(diff);
  const hours = Math.floor(span / 3600);
  const minutes = Math.floor((span % 3600) / 60);
  if (diff <= 0) {
    if (hours >= 24) return t('timeline.aired');
    return hours > 0
      ? t('timeline.airedHoursAgo', { count: hours })
      : t('timeline.airedMinutesAgo', { count: minutes });
  }
  if (hours >= 24) return t('timeline.inDaysHours', { days: Math.floor(hours / 24), hours: hours % 24 });
  if (hours > 0) return t('timeline.inHoursMinutes', { hours, minutes });
  return t('timeline.inMinutes', { count: Math.max(1, minutes) });
}

/** The entries the filters let through. */
export function filterSchedule<T extends ScheduleItem>(
  entries: T[],
  filters: ScheduleFilters,
  myList: Set<number>,
  dubKnown: Set<number>
): T[] {
  return entries.filter(
    (e) =>
      (!filters.format || e.format === filters.format) &&
      (!filters.myList || myList.has(e.media_id)) &&
      (!filters.dub || dubKnown.has(e.media_id))
  );
}

/** How many entries air in each `[start, end)` window. */
export function countPerDay(entries: ScheduleItem[], days: { start: number; end: number }[]): number[] {
  return days.map((day) => entries.filter((e) => e.airing_at >= day.start && e.airing_at < day.end).length);
}

/**
 * Where the NOW line goes in a day's list sorted by time: the index of the first entry still
 * to air (the list length when all have aired), or null when the day is not today.
 */
export function nowLineIndex(sorted: ScheduleItem[], now: number, isToday: boolean): number | null {
  if (!isToday) return null;
  const index = sorted.findIndex((e) => e.airing_at > now);
  return index === -1 ? sorted.length : index;
}

/** The next episode still to air among `ids` (belled or on the list), or null. */
export function upNext<T extends ScheduleItem>(entries: T[], ids: Set<number>, now: number): T | null {
  let best: T | null = null;
  for (const e of entries) {
    if (e.airing_at > now && ids.has(e.media_id) && (!best || e.airing_at < best.airing_at)) best = e;
  }
  return best;
}

/**
 * The weeks of a month for a calendar, Monday first: each cell is a day of the month or null
 * for the padding before the 1st and after the last day.
 */
export function monthWeeks(year: number, month: number): (number | null)[][] {
  const first = new Date(year, month, 1);
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const lead = (first.getDay() + 6) % 7;
  const cells: (number | null)[] = [...Array(lead).fill(null)];
  for (let d = 1; d <= daysInMonth; d++) cells.push(d);
  while (cells.length % 7 !== 0) cells.push(null);
  const weeks: (number | null)[][] = [];
  for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
  return weeks;
}
