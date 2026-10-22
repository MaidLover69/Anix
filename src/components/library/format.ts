import type { AppSettings, Media } from '../../types';
import { formatNumber, locale, t, type TKey } from '../../i18n';

/** Only mpv and vlc can open a file; the 'builtin' setting falls back to mpv. */
export function externalPlayerFor(settings: Pick<AppSettings, 'external_player'> | undefined): 'mpv' | 'vlc' {
  return settings?.external_player === 'vlc' ? 'vlc' : 'mpv';
}

export function playerLabel(player: 'mpv' | 'vlc'): string {
  return player === 'vlc' ? 'VLC' : 'mpv';
}

/** Accepts Unix seconds or milliseconds and returns milliseconds. */
export function toMillis(timestamp: number): number {
  return timestamp < 1e11 ? timestamp * 1000 : timestamp;
}

export function formatDate(timestamp: number): string {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return '';
  return new Date(toMillis(timestamp)).toLocaleDateString(locale(), {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
  });
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes <= 0) return '';
  const gb = bytes / (1024 * 1024 * 1024);
  const oneDecimal = { minimumFractionDigits: 1, maximumFractionDigits: 1 };
  if (gb >= 1) return t('libraryFormat.gigabytes', { size: formatNumber(gb, oneDecimal) });
  return t('libraryFormat.megabytes', { size: formatNumber(bytes / (1024 * 1024), oneDecimal) });
}

/**
 * Watched share as a whole percentage, or null when the stored duration cannot be trusted
 * (missing, zero, or shorter than the saved position).
 */
export function watchedPercent(positionMs: number, durationMs: number): number | null {
  if (!Number.isFinite(positionMs) || !Number.isFinite(durationMs)) return null;
  if (durationMs <= 0 || positionMs < 0) return null;
  if (positionMs > durationMs * 1.05) return null;
  return Math.min(100, Math.round((positionMs / durationMs) * 100));
}

export function categoryLabel(category: string | null | undefined): string {
  return category?.toLowerCase() === 'dub' ? t('common.dub') : t('common.sub');
}

export function watchCategory(category: string | null | undefined): 'sub' | 'dub' {
  return category?.toLowerCase() === 'dub' ? 'dub' : 'sub';
}

/** "NOT_YET_RELEASED" -> "Not yet released" */
export function sentenceCase(value: string | null | undefined): string {
  if (!value) return '';
  const text = value.replace(/_/g, ' ').toLowerCase();
  return text.charAt(0).toUpperCase() + text.slice(1);
}

const MEDIA_FORMAT_LABELS: Record<string, TKey> = {
  TV: 'libraryFormat.formatTv',
  TV_SHORT: 'libraryFormat.formatTvShort',
  MOVIE: 'libraryFormat.formatMovie',
  SPECIAL: 'libraryFormat.formatSpecial',
  OVA: 'libraryFormat.formatOva',
  ONA: 'libraryFormat.formatOna',
  MUSIC: 'libraryFormat.formatMusic',
  MANGA: 'libraryFormat.formatManga',
  NOVEL: 'libraryFormat.formatNovel',
  ONE_SHOT: 'libraryFormat.formatOneShot',
};

/** AniList format as it reads in a caption: "TV", "Movie", "OVA", "TV short". */
export function mediaFormatLabel(format: string | null | undefined): string {
  if (!format) return '';
  const key = MEDIA_FORMAT_LABELS[format.toUpperCase()];
  return key ? t(key) : sentenceCase(format);
}

const DOWNLOAD_STATUS_LABELS: Record<string, string> = {
  QUEUED: 'Queued',
  DOWNLOADING: 'Downloading',
  COMPLETED: 'Completed',
  FAILED: 'Failed',
};

export function downloadStatusLabel(status: string): string {
  return DOWNLOAD_STATUS_LABELS[status.toUpperCase()] ?? sentenceCase(status);
}

export const ANILIST_LIST_STATUSES: { value: string; label: string }[] = [
  { value: 'CURRENT', label: 'Watching' },
  { value: 'PLANNING', label: 'Planning' },
  { value: 'COMPLETED', label: 'Completed' },
  { value: 'PAUSED', label: 'Paused' },
  { value: 'DROPPED', label: 'Dropped' },
];

export function episodeLabel(episode: number): string {
  return t('common.episodeNumber', { number: episode });
}

/** Builds the `Media` shape `AnimeCard` needs from a stored list row. */
export function mediaFromListRow(row: {
  id: number;
  title: string | null;
  cover: string | null;
  format: string | null;
  averageScore: number | null;
  episodes?: number | null;
}): Media {
  return {
    id: row.id,
    id_mal: null,
    title: {
      english: row.title,
      romaji: row.title,
      native: null,
      user_preferred: row.title,
    },
    cover_image: {
      extra_large: row.cover,
      large: row.cover,
      color: null,
    },
    banner_image: null,
    description: null,
    format: row.format,
    status: null,
    episodes: row.episodes ?? null,
    duration: null,
    season: null,
    season_year: null,
    average_score: row.averageScore,
    mean_score: null,
    popularity: null,
    favourites: null,
    genres: [],
    is_adult: false,
  };
}

const FIELD_CLASS =
  'w-full px-3 rounded-md bg-bg-input border border-border text-[13px] text-text-primary placeholder:text-text-disabled select-text disabled:opacity-60';

/** Text fields in the library forms and dialogs. */
export const INPUT_CLASS = `${FIELD_CLASS} h-9`;
export const TEXTAREA_CLASS = `${FIELD_CLASS} py-2 resize-none`;

export const LABEL_CLASS = 'block mb-1.5 text-[13px] font-medium text-text-primary';

/**
 * Days in a row with at least one watched episode, by local calendar day. `current` is the
 * run ending today, or yesterday when nothing has been watched yet today. Days are stepped
 * with setDate, so DST changes don't break a run.
 */
export function watchStreak(timestamps: number[], now = new Date()): { current: number; longest: number } {
  const dayKey = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  const days = new Set<string>();
  const starts: number[] = [];
  for (const t of timestamps) {
    if (!Number.isFinite(t) || t <= 0) continue;
    const d = new Date(toMillis(t));
    const key = dayKey(d);
    if (days.has(key)) continue;
    days.add(key);
    starts.push(new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime());
  }
  starts.sort((a, b) => a - b);

  let longest = 0;
  let run = 0;
  let expected: string | null = null;
  for (const start of starts) {
    const day = new Date(start);
    run = dayKey(day) === expected ? run + 1 : 1;
    longest = Math.max(longest, run);
    day.setDate(day.getDate() + 1);
    expected = dayKey(day);
  }

  const cursor = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  if (!days.has(dayKey(cursor))) cursor.setDate(cursor.getDate() - 1);
  let current = 0;
  while (days.has(dayKey(cursor))) {
    current++;
    cursor.setDate(cursor.getDate() - 1);
  }
  return { current, longest };
}
