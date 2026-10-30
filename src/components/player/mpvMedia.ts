import type { MpvAction, MpvEvent, MpvTrack } from '../../hooks/useEmbeddedMpv';

/** The part of `TimeRanges` the timeline reads. */
interface Ranges {
  length: number;
  start: (index: number) => number;
  end: (index: number) => number;
}

/**
 * The embedded mpv player seen through the `HTMLMediaElement` members and events the player
 * UI uses (time, duration, pause, volume, speed, seeking, buffered, ended), so the controls,
 * progress saving, skip times and MPRIS work on both engines unchanged. Setters send requests
 * to mpv; the state follows mpv's own reports, like an element follows its media.
 */
export class MpvMedia extends EventTarget {
  private time = 0;
  private length = Number.NaN;
  private isPaused = true;
  private level = 1;
  private silent = false;
  private rate = 1;
  private cachedUntil = 0;
  private message: string | null = null;
  /** mpv holds playback to fill its cache: a stall, not a pause. */
  private stalled = false;
  private readonly send: (action: MpvAction) => void;
  ended = false;
  seeking = false;
  /** 0 until a file is open, then 4 (enough data), as on an element. */
  readyState = 0;
  defaultPlaybackRate = 1;
  tracks: MpvTrack[] = [];
  /** Counts the files mpv finished opening. */
  loadCount = 0;
  /** The proxy URL of the open stream, set by the engine when it loads one. */
  src: string | null = null;
  /** True from a reopen after the end until mpv reports the file loaded. */
  replaying = false;

  constructor(send: (action: MpvAction) => void) {
    super();
    this.send = send;
  }

  private fire(...names: string[]) {
    names.forEach((name) => this.dispatchEvent(new Event(name)));
  }

  get currentTime(): number {
    return this.time;
  }
  set currentTime(value: number) {
    if (!Number.isFinite(value)) return;
    const target = Math.max(0, Number.isFinite(this.length) ? Math.min(value, this.length) : value);
    const reopen = this.ended && this.src !== null;
    this.time = target;
    this.seeking = true;
    this.ended = false;
    // At the end mpv unloads the file (idle, no keep-open), so a seek has nothing to move in.
    if (reopen) {
      // Opened paused, as an element stays paused when seeking after the end.
      this.send({ action: 'set_pause', paused: this.isPaused });
      this.reopen(target);
    } else this.send({ action: 'seek', seconds: target, absolute: true });
    this.fire('seeking', 'timeupdate');
    if (reopen) this.fire('waiting');
  }

  get duration(): number {
    return this.length;
  }

  get paused(): boolean {
    return this.isPaused;
  }

  get error(): { message: string } | null {
    return this.message === null ? null : { message: this.message };
  }

  get volume(): number {
    return this.level;
  }
  set volume(value: number) {
    if (!Number.isFinite(value)) return;
    this.level = Math.min(1, Math.max(0, value));
    this.send({ action: 'set_volume', value: this.level * 100 });
    this.fire('volumechange');
  }

  get muted(): boolean {
    return this.silent;
  }
  set muted(value: boolean) {
    this.silent = value;
    this.send({ action: 'set_mute', muted: value });
    this.fire('volumechange');
  }

  get playbackRate(): number {
    return this.rate;
  }
  set playbackRate(value: number) {
    if (!(value > 0)) return;
    this.rate = value;
    this.send({ action: 'set_speed', value });
    this.fire('ratechange');
  }

  get buffered(): Ranges {
    const end = Math.max(this.time, this.cachedUntil);
    const has = this.readyState > 0 && end > this.time;
    return { length: has ? 1 : 0, start: () => this.time, end: () => end };
  }

  play(): Promise<void> {
    const reopen = this.ended && this.src !== null;
    this.send({ action: 'set_pause', paused: false });
    if (reopen) {
      this.time = 0;
      this.ended = false;
      this.reopen(0);
    }
    // Applied at once: mpv reports `pause` only when it changes, so a request that changes
    // nothing there would otherwise leave the controls showing the wrong state.
    this.setPaused(false);
    // Until mpv has opened the file again the controls show buffering, not a frozen frame.
    if (reopen) this.fire('waiting');
    return Promise.resolve();
  }

  pause(): void {
    this.send({ action: 'set_pause', paused: true });
    this.setPaused(true);
  }

  /** Opens the current stream again at `start`. */
  private reopen(start: number) {
    if (this.src === null) return;
    this.replaying = true;
    this.readyState = 0;
    this.send({ action: 'load', url: this.src, start: start > 0 ? start : null });
  }

  private setPaused(paused: boolean) {
    if (paused === this.isPaused) return;
    this.isPaused = paused;
    if (!paused) this.ended = false;
    if (paused) return this.fire('pause');
    // `playing` means frames are coming; while mpv fills its cache that waits for the cache.
    this.fire('play', ...(this.stalled ? [] : ['playing']));
  }

  /** Applies one report of the backend's `embedded-mpv` event. */
  handle(event: MpvEvent): void {
    switch (event.kind) {
      case 'property':
        return this.property(event.name, event.value);
      case 'loaded':
        this.tracks = event.tracks;
        this.readyState = 4;
        this.ended = false;
        this.message = null;
        this.loadCount += 1;
        this.stalled = false;
        this.fire('loadedmetadata', 'durationchange', 'canplay', 'trackschange');
        this.replaying = false;
        // mpv's pause state at load; it sends no property event for a value that did not change.
        if (typeof event.paused === 'boolean') this.setPaused(event.paused);
        return;
      case 'tracks':
        this.tracks = event.tracks;
        return this.fire('trackschange');
      case 'seeking':
        this.seeking = true;
        return this.fire('seeking');
      case 'seeked':
        this.seeking = false;
        return this.fire('seeked', this.stalled ? 'waiting' : isPlayingEvent(this.isPaused));
      case 'end':
        this.stalled = false;
        if (event.reason === 'eof') {
          this.ended = true;
          this.isPaused = true;
          if (Number.isFinite(this.length)) this.time = this.length;
          return this.fire('timeupdate', 'ended');
        }
        // A reopen that failed is over; the next stream starts from its own position.
        this.replaying = false;
        this.message = event.error;
        return this.fire('error');
      default:
        return;
    }
  }

  private property(name: string, value: number | boolean | null) {
    const number = typeof value === 'number' && Number.isFinite(value) ? value : null;
    switch (name) {
      case 'time-pos':
        if (number === null) return;
        this.time = number;
        return this.fire('timeupdate');
      case 'duration':
        this.length = number !== null && number > 0 ? number : Number.NaN;
        return this.fire('durationchange');
      case 'pause':
        return this.setPaused(value === true);
      case 'paused-for-cache':
        // Shown as buffering (a spinner), never as the user's pause.
        this.stalled = value === true;
        return this.fire(this.stalled ? 'waiting' : isPlayingEvent(this.isPaused));
      case 'demuxer-cache-time':
        this.cachedUntil = number ?? 0;
        return this.fire('progress');
      case 'volume':
        if (number === null) return;
        this.level = Math.min(1, Math.max(0, number / 100));
        return this.fire('volumechange');
      case 'mute':
        this.silent = value === true;
        return this.fire('volumechange');
      case 'speed':
        if (number === null || number === this.rate) return;
        this.rate = number;
        return this.fire('ratechange');
      default:
        return;
    }
  }
}

/** After a stall or a seek the element reports `playing`, or `canplay` while paused. */
function isPlayingEvent(paused: boolean): string {
  return paused ? 'canplay' : 'playing';
}
