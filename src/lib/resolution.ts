// Preferred resolution for streaming and downloads (pure rules, no imports).

export const STREAMING_RESOLUTION_VALUES = ['auto', '1080', '720', '480', '360'];
export const DOWNLOAD_QUALITY_VALUES = ['best', '1080p', '720p', '480p', '360p'];

const HEIGHTS = [1080, 720, 480, 360];

/** A setting ("720", "720p", 720) as a height; auto, best and anything unknown mean no constraint. */
export function wantedHeight(value: unknown): number | null {
  const height = Number.parseInt(String(value ?? '').trim(), 10);
  return HEIGHTS.includes(height) ? height : null;
}

/** The wanted height if offered, else the closest lower, else the closest higher. */
export function pickHeight(heights: number[], wanted: number | null): number | null {
  if (wanted === null) return null;
  const known = heights.filter((h) => h > 0);
  const lower = known.filter((h) => h <= wanted);
  if (lower.length) return Math.max(...lower);
  return known.length ? Math.min(...known) : null;
}

/** Index of the level hls.js starts at (the best bitrate at the picked height); -1 is Auto. */
export function startLevelIndex(levels: { height: number; bitrate: number }[], wanted: number | null): number {
  const height = pickHeight(levels.map((l) => l.height), wanted);
  if (height === null) return -1;
  let best = -1;
  levels.forEach((level, i) => {
    if (level.height === height && (best === -1 || level.bitrate > levels[best].bitrate)) best = i;
  });
  return best;
}
