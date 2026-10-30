/**
 * Short stable codes for error messages (`NET-403`, `CAT-TIMEOUT`, `SRC-VERIFY`), so a bug
 * report can quote one. Commands hand errors over as text, so the code is read from the
 * message. Same rules as `code_for_message` in `src-tauri/src/error.rs`.
 */

const CATALOGUE_NAMES = ['anilist', 'myanimelist', 'jikan', 'kitsu', 'shikimori', 'comick', 'imdb'];
const HTTP_STATUS = /\b(?:http|status)(?:\s+status)?(?:\s+code)?\s*:?\s*([1-5]\d\d)\b/i;

const domain = (m: string) => (CATALOGUE_NAMES.some((name) => m.toLowerCase().includes(name)) ? 'CAT' : 'NET');

function networkDetail(m: string): string | null {
  const lower = m.toLowerCase();
  if (lower.includes('timed out') || lower.includes('timeout')) return 'TIMEOUT';
  return HTTP_STATUS.exec(m)?.[1] ?? null;
}

function connectionDetail(m: string): string {
  const lower = m.toLowerCase();
  if (['dns', 'lookup', 'resolve'].some((w) => lower.includes(w))) return 'DNS';
  if (['tls', 'certificate', 'handshake', 'ssl'].some((w) => lower.includes(w))) return 'TLS';
  if (lower.includes('timed out') || lower.includes('timeout')) return 'TIMEOUT';
  return 'CONNECT';
}

/**
 * Whether a catalogue answered that it has no such anime (the backend's
 * `catalogue::model::is_not_found`: a parse error saying "not found"). It is shown as
 * "Anime not found" and not retried, unlike a lost connection.
 */
export function isNotFoundError(error: unknown): boolean {
  const m = String(error ?? '').trim();
  return m.startsWith('Parse error: ') && /not found/i.test(m);
}

export function errorCode(message: string): string {
  const m = message.trim();
  const after = (prefix: string) => (m.startsWith(prefix) ? m.slice(prefix.length) : null);
  let inner: string | null;
  if ((inner = after('Network error: ')) !== null) return `${domain(inner)}-${networkDetail(inner) ?? 'ERR'}`;
  if ((inner = after('Connection failed: ')) !== null) return `${domain(inner)}-${connectionDetail(inner)}`;
  if (m.startsWith('Database error: ')) return 'DB-ERR';
  if ((inner = after('Parse error: ')) !== null) return `${domain(inner)}-PARSE`;
  if ((inner = after('Provider error: ')) !== null) return `SRC-${networkDetail(inner) ?? 'ERR'}`;
  if (m === "Blocked by the site's anti-bot check") return 'SRC-CAPTCHA';
  if (m.endsWith(' needs verification')) return 'SRC-VERIFY';
  if (m === 'No sources found') return 'SRC-NONE';
  if (m.startsWith('Rate limited')) return 'NET-429';
  if (m === 'Auth required') return 'AUTH-LOGIN';
  inner = after('Other error: ') ?? m;
  const detail = networkDetail(inner);
  return detail ? `${domain(inner)}-${detail}` : 'APP-ERR';
}

/**
 * The `errors.*` catalog keys that can explain a message, most specific first: its code, the
 * code's HTTP status class (`CAT-403` → `CAT-4xx`), then its area's general key (`CAT-ERR`).
 * Status codes are open-ended, so a class key covers the ones without a text of their own.
 * None for a message without an error prefix: that is a sentence a command wrote for the user,
 * or one a screen already translated, and the general APP-ERR line would hide what it says.
 * `generic` false also leaves out APP-ERR for "Other error: …": that line only says "unexpected
 * problem", so it may stand in only where the raw message stays visible (ErrorState's Details).
 */
export function errorKeys(message: string, generic = true): string[] {
  const code = errorCode(message);
  if (code === 'APP-ERR' && (!generic || !message.trim().startsWith('Other error: '))) return [];
  const cut = code.indexOf('-');
  const area = code.slice(0, cut);
  const detail = code.slice(cut + 1);
  const keys = [code];
  if (/^\d{3}$/.test(detail)) keys.push(`${area}-${detail[0]}xx`);
  keys.push(`${area}-ERR`);
  return [...new Set(keys)].map((key) => `errors.${key}`);
}
