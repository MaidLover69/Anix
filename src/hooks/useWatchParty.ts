/**
 * The watch party on the player page. The host's player reports play, pause, seeks and its
 * position to the backend, which hands them to the guests; a guest's player follows the host
 * and corrects drift by seeking. Both engines are driven through the same media members and
 * engine play/pause the rest of the player uses, so web player and embedded mpv behave alike.
 * `usePartyFollow` (in the app layout) takes a guest to the host's episode.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useLocation, useNavigate } from 'react-router-dom';
import { watchPath } from './watchProgress';
import { ALIGN_LEAD_MS, DEAD_BAND_MS, hostPositionMs, isBufferedAhead, isNewHostState, needsAlign, pastEnd } from '../lib/watchParty';
import type { HostSample } from '../lib/watchParty';

export type PartyRole = 'none' | 'host' | 'guest';

export interface PartyStatus {
  role: PartyRole;
  code: string | null;
  addresses: string[];
  port: number;
  guests: number;
  locked: boolean;
  waiting: boolean;
  rtt_ms: number | null;
}

/** What the host plays (`party::protocol::Load`). */
export interface PartyLoad {
  anime_id: number;
  episode: number;
  category: 'sub' | 'dub';
  language: string | null;
  extension: string | null;
  server: string | null;
  duration_ms: number | null;
}

interface PlayState {
  seq: number;
  paused: boolean;
  position_ms: number;
  rate: number;
  age_ms: number;
}

type PartyEvent =
  | ({ kind: 'status' } & PartyStatus)
  | ({ kind: 'load' } & PartyLoad)
  | { kind: 'prepare'; seq: number; position_ms: number }
  | ({ kind: 'state' } & PlayState)
  | { kind: 'ended'; reason: 'left' | 'host_left' | 'lost' };

interface Snapshot {
  status: PartyStatus;
  load: PartyLoad | null;
  play: PartyEvent | null;
}

export const NO_PARTY: PartyStatus = {
  role: 'none',
  code: null,
  addresses: [],
  port: 0,
  guests: 0,
  locked: false,
  waiting: false,
  rtt_ms: null,
};

/** A programmatic play/pause/seek's event arriving within this window is not the user's. */
const QUIET_MS = 800;
type QuietKind = 'play' | 'pause' | 'seeked' | 'ratechange';
const QUIET_KINDS: QuietKind[] = ['play', 'pause', 'seeked', 'ratechange'];
/** A pause the engine did not ask for counts as a real one after this long. */
const HELD_PAUSE_MS = 2000;
const HEARTBEAT_MS = 1000;
const CHECK_MS = 500;

function onPartyEvent(handler: (event: PartyEvent) => void): () => void {
  let disposed = false;
  let unlisten: (() => void) | null = null;
  listen<PartyEvent>('party-event', ({ payload }) => handler(payload)).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });
  return () => {
    disposed = true;
    unlisten?.();
  };
}

const sleep = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

/** Resolves once the next seconds are buffered (or the media can play through), or after `ms`. */
async function waitBuffered(media: HTMLVideoElement, ms: number): Promise<void> {
  const until = performance.now() + ms;
  while (performance.now() < until) {
    // The mpv stand-in reports 4 from load on, so only its buffered range counts.
    if (media.readyState >= 3 && media instanceof HTMLMediaElement) return;
    const b = media.buffered;
    const ranges: Array<[number, number]> = [];
    for (let i = 0; i < b.length; i++) ranges.push([b.start(i), b.end(i)]);
    if (isBufferedAhead(ranges, media.currentTime, media.duration)) return;
    await sleep(100);
  }
}

/** Resolves on the first of `names`, or after `ms`. */
function waitFor(target: EventTarget, names: string[], ms: number): Promise<void> {
  return new Promise((resolve) => {
    const done = () => {
      window.clearTimeout(timer);
      names.forEach((name) => target.removeEventListener(name, done));
      resolve();
    };
    const timer = window.setTimeout(done, ms);
    names.forEach((name) => target.addEventListener(name, done));
  });
}

/**
 * Takes a guest to the host's episode, from any screen, and back to it when the guest opens
 * another one. Ends a hosted party when the host leaves the player, and any party when the
 * window is hidden to the tray: nobody would see it still running.
 */
export function usePartyFollow() {
  const navigate = useNavigate();
  const location = useLocation();
  const here = useRef(location);
  here.current = location;
  const follow = useCallback(
    (load: PartyLoad) => {
      const target = watchPath(load.anime_id, load.episode, load.category, null);
      const now = here.current;
      const sameEpisode = now.pathname === target.split('?')[0];
      if (sameEpisode && new URLSearchParams(now.search).get('cat') === load.category) return;
      navigate(target, { replace: now.pathname.startsWith('/watch/') });
    },
    [navigate]
  );
  useEffect(() => onPartyEvent((event) => event.kind === 'load' && follow(event)), [follow]);
  const route = location.pathname + location.search;
  useEffect(() => {
    const watching = here.current.pathname.startsWith('/watch/');
    invoke<Snapshot>('party_status')
      .then(({ status, load }) => {
        if (status.role === 'host' && !watching) return invoke('party_leave');
        if (status.role === 'guest' && watching && load) follow(load);
      })
      .catch(() => {});
  }, [route, follow]);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    listen('main-window-hidden', () => void invoke('party_leave').catch(() => {})).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}

export interface PartyEpisode {
  anilistId: number;
  episodeNumber: number;
  category: 'sub' | 'dub';
  language: string | null;
  /** The extension playing it; null for local files. */
  extension: string | null;
  server: string | null;
  /** Seconds; 0 while unknown. */
  duration: number;
}

export interface PartyControls {
  play: () => void;
  pause: () => void;
  /** The engine's own "playback was asked for" (true while it holds playback to buffer). */
  wantsPlay: () => boolean;
}

export interface WatchParty {
  status: PartyStatus;
  /** What the host plays, as last announced (guests). */
  hostLoad: PartyLoad | null;
  isHost: boolean;
  isGuest: boolean;
  offsetMs: number;
  setOffsetMs: (ms: number) => void;
  /** Brings a guest's player back to the host now. */
  catchUp: () => void;
  /** After hosting or joining from the dialog. */
  setStatus: (status: PartyStatus) => void;
}

export function useWatchParty(
  videoRef: RefObject<HTMLVideoElement>,
  episode: PartyEpisode,
  controls: PartyControls,
  onEnded: (reason: 'host_left' | 'lost') => void
): WatchParty {
  const [status, setStatus] = useState<PartyStatus>(NO_PARTY);
  const [hostLoad, setHostLoad] = useState<PartyLoad | null>(null);
  const [offsetMs, setOffsetMs] = useState(0);
  const statusRef = useRef(status);
  statusRef.current = status;
  const offsetRef = useRef(offsetMs);
  offsetRef.current = offsetMs;
  const controlsRef = useRef(controls);
  controlsRef.current = controls;
  const onEndedRef = useRef(onEnded);
  onEndedRef.current = onEnded;

  const run = useRef({
    quietUntil: { play: 0, pause: 0, seeked: 0, ratechange: 0 } as Record<QuietKind, number>,
    busy: false,
    /** The Prepare this guest answered; its start keeps the same number. */
    preparedSeq: -1,
    /** The host paused its own player to start with the guests. */
    hostStarting: false,
    token: 0,
    sample: null as HostSample | null,
    seq: -1,
    lastAlignAt: 0,
    lastBeat: 0,
    /** The last pause came from the engine holding playback to buffer, not the user. */
    heldPause: false,
    heldTimer: 0,
    hostTimer: 0,
    tick: { value: 0, at: 0 },
  });

  /** Ignores the next events of `kinds` (all when none given) for a moment: this hook caused them. */
  const quiet = (...kinds: QuietKind[]) => {
    const until = performance.now() + QUIET_MS;
    for (const kind of kinds.length ? kinds : QUIET_KINDS) run.current.quietUntil[kind] = until;
  };
  const isQuiet = (kind: QuietKind) => run.current.busy || performance.now() < run.current.quietUntil[kind];

  /** The playhead in ms; mpv reports it about five times a second, so it is extrapolated. */
  const livePositionMs = (media: HTMLVideoElement) => {
    const { tick } = run.current;
    const t = media.currentTime;
    if (media.paused || t !== tick.value) return t * 1000;
    return (t + Math.min(0.3, (performance.now() - tick.at) / 1000) * (media.playbackRate || 1)) * 1000;
  };

  const seekTo = async (media: HTMLVideoElement, ms: number) => {
    quiet();
    media.currentTime = Math.max(0, ms) / 1000;
    await waitFor(media, ['seeked'], 8000);
  };
  const whenLoaded = async (media: HTMLVideoElement) => {
    if (media.readyState === 0) await waitFor(media, ['loadedmetadata', 'canplay'], 30_000);
  };

  // Guest: move to where the host is, paused or starting exactly when the host gets there.
  const align = useCallback(async () => {
    const media = videoRef.current;
    const st = run.current;
    const sample = st.sample;
    if (!media || !sample) return;
    const token = ++st.token;
    st.busy = true;
    try {
      await whenLoaded(media);
      if (token !== st.token) return;
      const offset = offsetRef.current;
      if (sample.paused) {
        quiet();
        if (!media.paused) controlsRef.current.pause();
        const target = sample.positionMs + offset;
        if (pastEnd(target, media.duration)) return;
        if (Math.abs(media.currentTime * 1000 - target) > DEAD_BAND_MS) await seekTo(media, target);
        return;
      }
      if (media.playbackRate !== sample.rate) {
        quiet();
        media.playbackRate = sample.rate;
      }
      let lead = ALIGN_LEAD_MS;
      for (let attempt = 0; attempt < 3; attempt++) {
        const target = hostPositionMs(sample, performance.now() + lead) + offset;
        if (pastEnd(target, media.duration)) {
          // A shorter copy waits at its end for the host's next episode.
          if (pastEnd(hostPositionMs(sample, performance.now()) + offset, media.duration)) {
            quiet();
            if (!media.paused) controlsRef.current.pause();
          } else if (media.paused) {
            // Only the start lead is past the end: play on to the end with the host.
            quiet();
            controlsRef.current.play();
          }
          return;
        }
        quiet();
        if (!media.paused) controlsRef.current.pause();
        if (Math.abs(media.currentTime * 1000 - target) > 50) await seekTo(media, target);
        if (token !== st.token) return;
        const wait = (target - offset - hostPositionMs(sample, performance.now())) / sample.rate;
        if (wait >= 0) {
          await sleep(wait);
          if (token !== st.token) return;
          quiet();
          controlsRef.current.play();
          return;
        }
        // The seek took longer than the lead: aim further ahead.
        lead *= 2;
      }
      quiet();
      controlsRef.current.play();
    } finally {
      if (token === st.token) {
        st.busy = false;
        st.lastAlignAt = performance.now();
        quiet();
      }
    }
  }, [videoRef]);

  // Guest: seek to the prepared position paused, then tell the host it is ready.
  const prepare = useCallback(
    async (seq: number, positionMs: number) => {
      const media = videoRef.current;
      const st = run.current;
      if (!media) return;
      st.sample = { paused: true, positionMs, rate: media.playbackRate || 1, ageMs: 0, receivedAt: performance.now() };
      st.seq = seq;
      st.preparedSeq = seq;
      const token = ++st.token;
      st.busy = true;
      try {
        await whenLoaded(media);
        if (token !== st.token) return;
        quiet();
        if (!media.paused) controlsRef.current.pause();
        const target = positionMs + offsetRef.current;
        if (Math.abs(media.currentTime * 1000 - target) > 50) await seekTo(media, target);
        await waitBuffered(media, 10_000);
        if (token !== st.token) return;
        invoke('party_ready', { seq }).catch(() => {});
      } finally {
        if (token === st.token) {
          st.busy = false;
          st.lastAlignAt = performance.now();
          quiet();
        }
      }
    },
    [videoRef]
  );

  const handle = useCallback(
    (event: PartyEvent) => {
      const st = run.current;
      switch (event.kind) {
        case 'status':
          setStatus({ ...event });
          return;
        case 'load':
          setHostLoad({ ...event });
          return;
        case 'prepare':
          if (statusRef.current.role === 'guest') void prepare(event.seq, event.position_ms);
          return;
        case 'state': {
          const sample = {
            paused: event.paused,
            positionMs: event.position_ms,
            rate: event.rate,
            ageMs: event.age_ms,
            receivedAt: performance.now(),
          };
          if (statusRef.current.role === 'host') {
            // The prepared start, scheduled so the guests start with it.
            window.clearTimeout(st.hostTimer);
            st.hostTimer = window.setTimeout(() => {
              st.hostStarting = false;
              quiet('play');
              controlsRef.current.play();
            }, Math.max(0, -event.age_ms));
            return;
          }
          const changed = isNewHostState(st.seq, st.preparedSeq, event.seq);
          st.preparedSeq = -1;
          st.sample = sample;
          st.seq = event.seq;
          if (changed) void align();
          return;
        }
        case 'ended':
          st.sample = null;
          st.token += 1;
          // An align or prepare cut short by the token leaves these set.
          st.busy = false;
          st.hostStarting = false;
          setStatus(NO_PARTY);
          setHostLoad(null);
          setOffsetMs(0);
          if (event.reason !== 'left') onEndedRef.current(event.reason);
          return;
      }
    },
    [align, prepare]
  );

  // The party as it is when this player opens, then its events.
  useEffect(() => {
    let disposed = false;
    invoke<Snapshot>('party_status')
      .then((snapshot) => {
        if (disposed) return;
        setStatus(snapshot.status);
        statusRef.current = snapshot.status;
        setHostLoad(snapshot.load);
        if (snapshot.status.role === 'guest' && snapshot.play) handle(snapshot.play);
      })
      .catch(() => {});
    const stop = onPartyEvent(handle);
    return () => {
      disposed = true;
      stop();
      window.clearTimeout(run.current.hostTimer);
      run.current.token += 1;
    };
  }, [handle]);

  // The player's own events: reported by a host, undone by a guest.
  useEffect(() => {
    const media = videoRef.current;
    if (!media) return;
    const st = run.current;
    const report = (paused: boolean, heartbeat = false) =>
      invoke('party_report', {
        paused,
        positionMs: Math.round(media.currentTime * 1000),
        rate: media.playbackRate || 1,
        heartbeat,
      }).catch(() => {});
    const onPlay = () => {
      const role = statusRef.current.role;
      if (role === 'none' || isQuiet('play')) return;
      if (st.heldPause) {
        st.heldPause = false;
        window.clearTimeout(st.heldTimer);
        return;
      }
      if (role === 'guest') return void align();
      // The host starts with everyone: hold here until the guests are ready.
      holdForStart();
    };
    /** Host: pause this player until the prepared start, which the backend schedules. */
    const holdForStart = () => {
      // A start already scheduled would play this player before the guests are ready again.
      window.clearTimeout(st.hostTimer);
      if (!media.paused) {
        quiet('pause');
        controlsRef.current.pause();
      }
      st.hostStarting = true;
      void report(false);
    };
    const onPause = () => {
      const role = statusRef.current.role;
      if (role === 'none' || isQuiet('pause')) return;
      const settle = () => {
        if (statusRef.current.role === 'guest') return void align();
        window.clearTimeout(st.hostTimer);
        st.hostStarting = false;
        // The next play is the user's, even inside the scheduled start's quiet window.
        st.quietUntil.play = 0;
        void report(true);
      };
      // Still wanted: the engine holds playback to buffer, or the stream ended. A pause that
      // outlasts a short hold (one from outside the engine) counts after all.
      if (controlsRef.current.wantsPlay() || media.ended) {
        st.heldPause = true;
        window.clearTimeout(st.heldTimer);
        st.heldTimer = window.setTimeout(() => {
          if (!st.heldPause || !media.paused) return;
          st.heldPause = false;
          settle();
        }, HELD_PAUSE_MS);
        return;
      }
      settle();
    };
    const onSeeked = () => {
      const role = statusRef.current.role;
      if (role === 'none' || isQuiet('seeked')) return;
      if (role === 'guest') return void align();
      // Paused by the user, or only held for a start: then the seek still plays.
      if (media.paused && !st.hostStarting) return void report(true);
      holdForStart();
    };
    const onRate = () => {
      if (statusRef.current.role === 'host' && !isQuiet('ratechange') && !media.paused) void report(false, true);
    };
    const onTime = () => {
      st.tick = { value: media.currentTime, at: performance.now() };
      if (statusRef.current.role !== 'host' || media.paused) return;
      if (performance.now() - st.lastBeat < HEARTBEAT_MS) return;
      st.lastBeat = performance.now();
      void report(false, true);
    };
    media.addEventListener('play', onPlay);
    media.addEventListener('pause', onPause);
    media.addEventListener('seeked', onSeeked);
    media.addEventListener('ratechange', onRate);
    media.addEventListener('timeupdate', onTime);
    // Guest: drift check.
    const timer = window.setInterval(() => {
      const sample = st.sample;
      if (statusRef.current.role !== 'guest' || !sample || st.busy || media.readyState === 0) return;
      // Held by the engine to buffer: it resumes by itself, then this corrects.
      if (st.heldPause) return;
      const now = performance.now();
      const check = {
        expectedMs: hostPositionMs(sample, now) + offsetRef.current,
        actualMs: livePositionMs(media),
        actualPaused: media.paused,
        hostPaused: sample.paused,
        sinceAlignMs: now - st.lastAlignAt,
      };
      // A start still ahead is handled by the align that scheduled it.
      if (!sample.paused && hostPositionMs(sample, now) < sample.positionMs && media.paused) return;
      if (needsAlign(check)) void align();
    }, CHECK_MS);
    return () => {
      media.removeEventListener('play', onPlay);
      media.removeEventListener('pause', onPause);
      media.removeEventListener('seeked', onSeeked);
      media.removeEventListener('ratechange', onRate);
      media.removeEventListener('timeupdate', onTime);
      window.clearInterval(timer);
      window.clearTimeout(st.heldTimer);
      // The host's player closes (Back, another episode): the guests stop with it.
      if (statusRef.current.role === 'host') void report(true);
      st.hostStarting = false;
    };
  }, [videoRef, align]);

  // Host: announce what plays.
  const durationMs = episode.duration > 0 ? Math.round(episode.duration * 1000) : null;
  useEffect(() => {
    if (status.role !== 'host') return;
    invoke('party_load', {
      load: {
        anime_id: episode.anilistId,
        episode: episode.episodeNumber,
        category: episode.category,
        language: episode.language && /^[a-z]{2,3}$/.test(episode.language) ? episode.language : null,
        extension: episode.extension,
        server: episode.server ? episode.server.slice(0, 64) : null,
        duration_ms: durationMs,
      },
    })
      .then(() => {
        // Hosting mid-episode, or an episode that opened without playing: the guests learn
        // where the host is. The backend ignores it once this episode has a report.
        const media = videoRef.current;
        if (!media) return;
        return invoke('party_report', {
          paused: media.paused,
          positionMs: Math.round(media.currentTime * 1000),
          rate: media.playbackRate || 1,
          heartbeat: true,
        });
      })
      .catch(() => {});
  }, [
    videoRef,
    status.role,
    episode.anilistId,
    episode.episodeNumber,
    episode.category,
    episode.language,
    episode.extension,
    episode.server,
    durationMs,
  ]);

  const catchUp = useCallback(() => {
    run.current.lastAlignAt = 0;
    void align();
  }, [align]);

  // A changed offset applies at once.
  useEffect(() => {
    if (statusRef.current.role === 'guest') void align();
  }, [offsetMs, align]);

  return {
    status,
    hostLoad,
    isHost: status.role === 'host',
    isGuest: status.role === 'guest',
    offsetMs,
    setOffsetMs,
    catchUp,
    setStatus,
  };
}
