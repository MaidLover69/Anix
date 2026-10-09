/** `m:ss`, or `h:mm:ss` from an hour on. */
export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00';
  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = (total % 60).toString().padStart(2, '0');
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, '0')}:${secs}`
    : `${minutes}:${secs}`;
}

/** Episode numbers are floats on the wire (12, 12.5). */
export function sameEpisode(a: number, b: number): boolean {
  return Math.abs(a - b) < 0.01;
}
