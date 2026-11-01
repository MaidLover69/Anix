/** Dotted app versions ("1.2.0", "v1.3.0-rc1"). No imports, so node test scripts can load it. */

/**
 * True when `candidate` is a higher dotted version than `current` ("1.2.0" > "1.1.9"). A
 * pre-release ("1.3.0-rc.1") is lower than its final release and pre-releases compare as in
 * semver; build metadata after `+` is ignored. Read the same way as `is_newer_version` in Rust.
 */
export function isNewerVersion(candidate: string, current: string): boolean {
  const parse = (v: string) => {
    const s = v.trim().replace(/^v/i, '').split('+')[0];
    const dash = s.indexOf('-');
    const core = (dash < 0 ? s : s.slice(0, dash))
      .split('.')
      .map((part) => parseInt(part, 10))
      .map((n) => (Number.isFinite(n) ? n : 0));
    return { core, pre: dash < 0 ? null : s.slice(dash + 1).split('.') };
  };
  const a = parse(candidate);
  const b = parse(current);
  for (let i = 0; i < Math.max(a.core.length, b.core.length, 3); i++) {
    const diff = (a.core[i] ?? 0) - (b.core[i] ?? 0);
    if (diff !== 0) return diff > 0;
  }
  if (!a.pre || !b.pre) return !a.pre && !!b.pre;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    const x = a.pre[i];
    const y = b.pre[i];
    if (x === undefined || y === undefined) return y === undefined;
    if (x === y) continue;
    const xNum = /^\d+$/.test(x);
    const yNum = /^\d+$/.test(y);
    // Semver: numeric identifiers sort numerically and below alphanumeric ones.
    if (xNum && yNum) return Number(x) > Number(y);
    if (xNum !== yNum) return yNum;
    return x > y;
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

/**
 * The update window opens by itself once for each newer release: `prompted` is the version it
 * last opened for (`update_prompted_version`). Never over the player; the next screen will do.
 */
export function shouldPromptUpdate(latest: string | null | undefined, prompted: unknown, path: string): boolean {
  if (!latest || path.startsWith('/watch')) return false;
  if (typeof prompted !== 'string' || prompted.trim() === '') return true;
  return isNewerVersion(latest, prompted);
}
