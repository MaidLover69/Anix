/**
 * Which subtitle files the embedded mpv player loads. mpv downloads a file before it can show
 * it, and adding every file the extension lists (about nine on some sites) at load time made
 * starts take close to a minute. So only the shown file is added when a video opens, and the
 * others when the user picks them. Pure functions, so they can be tested without the app.
 */
import type { MpvTrack } from '../hooks/useEmbeddedMpv';

/** What to do with subtitles once mpv opened a file. */
export type SubtitleAtLoad =
  | { kind: 'add'; index: number }
  | { kind: 'off' }
  /** Keep what mpv shows by itself (a track inside the video file, or none). */
  | { kind: 'follow' };

/** The extension file's index of an `ext:<index>` option id, else null. */
export function providerIndex(id: string): number | null {
  const match = /^ext:(\d+)$/.exec(id);
  return match ? Number(match[1]) : null;
}

/**
 * The subtitle step at load. `wanted` is the current selection, or undefined before the first
 * file of the episode; then the default file is shown, or none for a dub.
 */
export function subtitleAtLoad(
  wanted: string | undefined,
  offId: string,
  count: number,
  defaultIndex: number,
  startOff: boolean
): SubtitleAtLoad {
  if (wanted === undefined) {
    if (startOff) return { kind: 'off' };
    return defaultIndex >= 0 && defaultIndex < count ? { kind: 'add', index: defaultIndex } : { kind: 'follow' };
  }
  if (wanted === offId) return { kind: 'off' };
  const index = providerIndex(wanted);
  // A track of the file itself may have a new id in a new file, so mpv's own choice is kept.
  return index !== null && index < count ? { kind: 'add', index } : { kind: 'follow' };
}

/** mpv's id of the track added from `url`, once mpv lists it. */
export function addedTrackId(tracks: MpvTrack[], url: string): number | null {
  return tracks.find((t) => t.type === 'sub' && t.id !== null && t.external_filename === url)?.id ?? null;
}

/** The video file's own subtitle tracks (added files are listed from the extension's list). */
export function fileSubtitleTracks(tracks: MpvTrack[]): MpvTrack[] {
  return tracks.filter((t) => t.type === 'sub' && t.id !== null && !t.external_filename);
}

/**
 * The track mpv must switch to so it shows the user's latest choice, or undefined when it
 * already does (or the choice is not known to mpv yet). Files are added in the background, so a
 * download that finishes after the user picked something else can select itself; this undoes
 * that. `wanted` is `off`, `ext:<index>` (with `urlOf` giving its proxy URL) or `mpv:<id>`.
 */
export function subtitleCorrection(
  tracks: MpvTrack[],
  wanted: string,
  urlOf: (index: number) => string | null
): number | null | undefined {
  const shown = tracks.find((t) => t.type === 'sub' && t.selected)?.id ?? null;
  let target: number | null;
  if (wanted === 'off') target = null;
  else if (wanted.startsWith('mpv:')) target = Number(wanted.slice(4));
  else {
    const index = providerIndex(wanted);
    const url = index === null ? null : urlOf(index);
    if (url === null) return undefined;
    target = addedTrackId(tracks, url);
    // Still downloading: its own select flag shows it when it is done.
    if (target === null) return undefined;
  }
  return shown === target ? undefined : target;
}
