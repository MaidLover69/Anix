/** Dotted app versions ("1.2.0", "v1.3.0-rc1"). No imports, so node test scripts can load it. */

/** True when `candidate` is a higher dotted version than `current` ("1.2.0" > "1.1.9"). */
export function isNewerVersion(candidate: string, current: string): boolean {
  const parse = (v: string) =>
    v
      .trim()
      .replace(/^v/i, '')
      .split(/[.+-]/)
      .map((part) => parseInt(part, 10))
      .map((n) => (Number.isFinite(n) ? n : 0));
  const a = parse(candidate);
  const b = parse(current);
  for (let i = 0; i < Math.max(a.length, b.length, 3); i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0);
    if (diff !== 0) return diff > 0;
  }
  return false;
}

/**
 * The setup screen opens on a first start (nothing saved) and once after each upgrade: when
 * the version it was last seen in (`setup_seen_version`) is older than the running one.
 */
export function needsSetup(seenVersion: unknown, current: string): boolean {
  if (typeof seenVersion !== 'string' || seenVersion.trim() === '') return true;
  return isNewerVersion(current, seenVersion);
}
