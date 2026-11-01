/**
 * The level Medium's "LEVEL UP!" toast last saw. Kept outside the badge: the sidebar unmounts on
 * the player, and a level gained there is announced once back on an ordinary screen, never over
 * the video. Its own module so the sidebar can forget it without loading FunBits.
 */
let seenLevel: number | null = null;

export function getSeenLevel(): number | null {
  return seenLevel;
}

export function setSeenLevel(level: number): void {
  seenLevel = level;
}

/** Meme mode went to Low: levels gained meanwhile are not announced when it comes back. */
export function forgetSeenLevel(): void {
  seenLevel = null;
}
