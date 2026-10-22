/**
 * Anime whose dub Anix has seen listed by a plugin (on their details page), so the Schedule
 * can offer "Dub available". Kept on this computer only; nothing is known about the rest.
 */
const STORAGE_KEY = 'anix-dub-known';
const LIMIT = 1000;

export function knownDubs(): Set<number> {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '[]');
    return new Set(Array.isArray(parsed) ? parsed.filter((id): id is number => typeof id === 'number') : []);
  } catch {
    return new Set();
  }
}

export function rememberDub(mediaId: number) {
  const known = knownDubs();
  if (known.has(mediaId)) return;
  try {
    // Newest last; the oldest drop off past the limit.
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify([...known, mediaId].slice(-LIMIT)));
  } catch {
    // Storage can be unavailable; the filter then knows less.
  }
}
