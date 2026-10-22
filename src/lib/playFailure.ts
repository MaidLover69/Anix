/**
 * A failed stream lookup as the player shows it. The backend sends the reason in plain words
 * on the first line and the per-provider list after it (`get_sources_for_episode`); other
 * errors are one line, shown as they are.
 */
export interface PlayFailure {
  reason: string;
  /** The raw per-provider list, for a Details section; null when there is none. */
  details: string | null;
}

export function splitPlayFailure(message: string): PlayFailure {
  const text = message.trim();
  const cut = text.indexOf('\n');
  const reason = (cut < 0 ? text : text.slice(0, cut)).trim();
  const details = cut < 0 ? null : text.slice(cut + 1).trim() || null;
  return { reason, details };
}
