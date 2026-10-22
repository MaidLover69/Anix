import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { RefObject } from 'react';
import { buildSubtitleUrl } from '../lib/streamProxy';
import type { StreamItem, SubtitleItem } from '../types';
import type { MediaTrackOption } from './usePlayerEngine';
import { useLanguagePrefs } from './useLanguagePrefs';
import { normalizeLanguage } from '../lib/languages';
import { t } from '../i18n';

export const SUBTITLES_OFF = 'off';

/** A subtitle file the user dropped on the player, already converted to WebVTT. */
export interface LocalSubtitle {
  label: string;
  /** A blob URL. */
  url: string;
}

export interface SubtitleOption {
  /**
   * `ext:<index>` for a provider subtitle file, `file:<index>` for a dropped file,
   * `hls:<id>` for a rendition in the manifest.
   */
  id: string;
  label: string;
  format: string;
  isDefault: boolean;
}

interface SubtitleTracksOptions {
  videoRef: RefObject<HTMLVideoElement>;
  /** Subtitle files supplied by the provider. */
  subtitles: SubtitleItem[];
  /** The stream being played; its referer and headers are needed to fetch the files. */
  stream: StreamItem | null;
  proxyBase: string;
  /** True while hls.js drives the element; it then manages the manifest's text tracks. */
  usesHls: boolean;
  /** Subtitle renditions inside the HLS manifest. */
  embeddedTracks: MediaTrackOption[];
  embeddedTrack: number;
  selectEmbeddedTrack: (id: number) => void;
  delayMs: number;
  /** Dubbed audio: subtitle files stay available but are not switched on by default. */
  startOff?: boolean;
  /** Files dropped on the player. */
  localFiles?: LocalSubtitle[];
}

/** Index of the first file in the most wanted subtitle language, else the provider's default; -1 for none. */
export function defaultSubtitleIndex(subtitles: SubtitleItem[], wanted: string[]): number {
  if (subtitles.length === 0) return -1;
  const codes = subtitles.map((sub) => normalizeLanguage(sub.language));
  let index = -1;
  for (const code of wanted) {
    index = codes.indexOf(code);
    if (index !== -1) break;
  }
  if (index === -1) index = subtitles.findIndex((sub) => sub.is_default);
  return index === -1 ? 0 : index;
}

function defaultSelection(subtitles: SubtitleItem[], wanted: string[]): string {
  const index = defaultSubtitleIndex(subtitles, wanted);
  return index === -1 ? SUBTITLES_OFF : `ext:${index}`;
}

/**
 * One subtitle selection for the player. A provider file is rendered as a single `<track>`
 * (see `trackSrc`), a manifest rendition is switched through hls.js, and the two are never
 * shown together.
 */
export function useSubtitleTracks({
  videoRef,
  subtitles,
  stream,
  proxyBase,
  usesHls,
  embeddedTracks,
  embeddedTrack,
  selectEmbeddedTrack,
  delayMs,
  startOff = false,
  localFiles = [],
}: SubtitleTracksOptions) {
  const wantedLanguages = useLanguagePrefs().sub;
  const [selectedId, setSelectedId] = useState(() =>
    startOff ? SUBTITLES_OFF : defaultSelection(subtitles, wantedLanguages)
  );
  const selectedRef = useRef(selectedId);
  selectedRef.current = selectedId;

  const options = useMemo<SubtitleOption[]>(
    () => [
      ...subtitles.map((sub, index) => ({
        id: `ext:${index}`,
        label: sub.language || t('useSubtitleTracks.trackNumber', { number: index + 1 }),
        format: sub.format || 'vtt',
        isDefault: !!sub.is_default,
      })),
      ...localFiles.map((file, index) => ({
        id: `file:${index}`,
        label: file.label,
        format: 'file',
        isDefault: false,
      })),
      ...embeddedTracks.map((track) => ({
        id: `hls:${track.id}`,
        label: track.label,
        format: 'hls',
        isDefault: false,
      })),
    ],
    [subtitles, localFiles, embeddedTracks]
  );

  // hls.js may switch a manifest rendition on by itself (a DEFAULT track). Follow it, unless
  // a provider file is selected, which takes precedence.
  useEffect(() => {
    const current = selectedRef.current;
    if (embeddedTrack >= 0) {
      if (current.startsWith('ext:') || current.startsWith('file:')) selectEmbeddedTrack(-1);
      else setSelectedId(`hls:${embeddedTrack}`);
    } else if (current.startsWith('hls:')) {
      setSelectedId(SUBTITLES_OFF);
    }
  }, [embeddedTrack, selectEmbeddedTrack]);

  const select = useCallback(
    (id: string) => {
      setSelectedId(id);
      selectEmbeddedTrack(id.startsWith('hls:') ? Number(id.slice(4)) : -1);
    },
    [selectEmbeddedTrack]
  );

  // Subtitle tracks inside a directly played file can be switched on by the webview itself;
  // only our own `<track>` may be visible then. With hls.js there are no such tracks, and
  // it keeps the manifest's renditions exclusive on its own.
  useEffect(() => {
    const video = videoRef.current;
    if (!video || usesHls) return;
    const tracks = video.textTracks;
    const enforce = () => {
      // The second subtitle is a `metadata` track and never rendered by the webview.
      const own = video.querySelector<HTMLTrackElement>('track[kind="subtitles"]')?.track ?? null;
      for (let i = 0; i < tracks.length; i++) {
        const track = tracks[i];
        const visible = track.kind === 'subtitles' || track.kind === 'captions';
        if (visible && track !== own && track.mode !== 'disabled') track.mode = 'disabled';
      }
    };
    enforce();
    tracks.addEventListener('addtrack', enforce);
    tracks.addEventListener('change', enforce);
    return () => {
      tracks.removeEventListener('addtrack', enforce);
      tracks.removeEventListener('change', enforce);
    };
  }, [videoRef, usesHls]);

  /** The WebVTT URL of a provider file or a dropped file; null for anything else. */
  const fileSrc = useCallback(
    (id: string): string | null => {
      if (id.startsWith('file:')) return localFiles[Number(id.slice(5))]?.url ?? null;
      const external = id.startsWith('ext:') ? subtitles[Number(id.slice(4))] : undefined;
      return external ? buildSubtitleUrl(proxyBase, external, stream) : null;
    },
    [localFiles, subtitles, proxyBase, stream]
  );
  const trackSrc = fileSrc(selectedId);

  // Subtitle delay: shift the cues of whichever track is showing. Cues can arrive late
  // (a file still loading, HLS subtitle segments), so the shift is re-checked as time moves.
  const shiftRef = useRef<{
    track: TextTrack | null;
    originals: Map<TextTrackCue, [number, number]>;
    count: number;
    delayMs: number;
  }>({ track: null, originals: new Map(), count: 0, delayMs: 0 });

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const apply = () => {
      let showing: TextTrack | null = null;
      for (let i = 0; i < video.textTracks.length; i++) {
        if (video.textTracks[i].mode === 'showing') {
          showing = video.textTracks[i];
          break;
        }
      }
      const shift = shiftRef.current;
      if (showing !== shift.track) {
        shift.track = showing;
        shift.originals = new Map();
        shift.count = 0;
      }
      const cues = showing?.cues;
      if (!cues) return;
      if (cues.length === shift.count && delayMs === shift.delayMs) return;

      const offset = delayMs / 1000;
      // Copy first: changing a cue's time may reorder the live list.
      const snapshot: TextTrackCue[] = [];
      for (let i = 0; i < cues.length; i++) snapshot.push(cues[i]);
      for (const cue of snapshot) {
        let original = shift.originals.get(cue);
        if (!original) {
          original = [cue.startTime, cue.endTime];
          shift.originals.set(cue, original);
        }
        const startTime = Math.max(0, original[0] + offset);
        const endTime = Math.max(startTime, original[1] + offset);
        if (cue.startTime !== startTime) cue.startTime = startTime;
        if (cue.endTime !== endTime) cue.endTime = endTime;
      }
      shift.count = cues.length;
      shift.delayMs = delayMs;
    };

    apply();
    video.addEventListener('timeupdate', apply);
    return () => video.removeEventListener('timeupdate', apply);
  }, [videoRef, delayMs]);

  return { options, selectedId, select, trackSrc, fileSrc };
}
