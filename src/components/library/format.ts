import type { AppSettings, Media } from '../../types';

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
  return new Date(toMillis(timestamp)).toLocaleDateString(undefined, {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
  });
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes <= 0) return '';
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
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
  return category?.toLowerCase() === 'dub' ? 'Dub' : 'Sub';
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

const MEDIA_FORMAT_LABELS: Record<string, string> = {
  TV: 'TV',
  TV_SHORT: 'TV short',
  MOVIE: 'Movie',
  SPECIAL: 'Special',
  OVA: 'OVA',
  ONA: 'ONA',
  MUSIC: 'Music',
  MANGA: 'Manga',
  NOVEL: 'Novel',
  ONE_SHOT: 'One-shot',
};

/** AniList format as it reads in a caption: "TV", "Movie", "OVA", "TV short". */
export function mediaFormatLabel(format: string | null | undefined): string {
  if (!format) return '';
  return MEDIA_FORMAT_LABELS[format.toUpperCase()] ?? sentenceCase(format);
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
  return `Episode ${episode}`;
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
