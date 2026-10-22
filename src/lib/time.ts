/**
 * Clock times as the "Time format" setting (Settings › Appearance) asks for them. Schedule, the
 * player clock and anything else that shows a time of day goes through `formatTime`, so the
 * setting changes every place at once. Kept free of React so a node script can check it.
 */

/** 'system' writes times as the locale does; the others force a 12- or 24-hour clock. */
export type TimeFormat = 'system' | '12h' | '24h';

export function isTimeFormat(value: unknown): value is TimeFormat {
  return value === 'system' || value === '12h' || value === '24h';
}

/** `Intl` options with the setting's hour cycle added (none for 'system'). */
export function timeOptions(format: TimeFormat | undefined, options: Intl.DateTimeFormatOptions = {}): Intl.DateTimeFormatOptions {
  if (format === '12h') return { ...options, hourCycle: 'h12' };
  // h23 so midnight reads 00:15, not 24:15.
  if (format === '24h') return { ...options, hourCycle: 'h23' };
  return options;
}

/**
 * The time of day of `when` (a Date or epoch milliseconds), hours and minutes by default.
 * `options` may add a weekday or date; the hour cycle always follows `format`.
 */
export function formatTime(
  when: Date | number,
  format: TimeFormat | undefined,
  options: Intl.DateTimeFormatOptions = { hour: 'numeric', minute: '2-digit' },
  locale?: string,
): string {
  return new Intl.DateTimeFormat(locale, timeOptions(format, options)).format(when);
}
