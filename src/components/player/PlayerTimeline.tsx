import React, { useEffect, useState } from 'react';
import type { RefObject } from 'react';
import { Slider } from './Slider';
import { formatTime } from './format';

interface PlayerTimelineProps {
  videoRef: RefObject<HTMLVideoElement>;
  /** Seconds; 0 while unknown. */
  duration: number;
  onSeek: (time: number) => void;
}

const TIME_EVENTS = ['timeupdate', 'seeking', 'seeked', 'progress', 'loadedmetadata', 'emptied'] as const;

/**
 * Timecodes and seek bar. This is the only component that follows the playhead, so the
 * several `timeupdate` events per second re-render nothing else.
 */
export const PlayerTimeline: React.FC<PlayerTimelineProps> = React.memo(({ videoRef, duration, onSeek }) => {
  const [time, setTime] = useState(0);
  const [buffered, setBuffered] = useState(0);
  // Position under the pointer while dragging; the seek happens on release.
  const [scrub, setScrub] = useState<number | null>(null);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const sync = () => {
      const current = video.currentTime;
      setTime(current);
      let end = 0;
      for (let i = 0; i < video.buffered.length; i++) {
        if (video.buffered.start(i) <= current + 0.5 && video.buffered.end(i) >= current) {
          end = video.buffered.end(i);
          break;
        }
      }
      setBuffered(end);
    };

    sync();
    TIME_EVENTS.forEach((name) => video.addEventListener(name, sync));
    return () => TIME_EVENTS.forEach((name) => video.removeEventListener(name, sync));
  }, [videoRef]);

  const shown = scrub ?? time;

  return (
    <div className="flex items-center gap-3 [text-shadow:0_1px_2px_rgb(0_0_0/0.6)]">
      <span className="w-14 text-xs tabular-nums text-white">{formatTime(shown)}</span>
      <Slider
        className="flex-1"
        label="Seek"
        value={Math.min(shown, duration)}
        max={duration}
        keyStep={5}
        buffered={buffered}
        valueText={`${formatTime(shown)} of ${formatTime(duration)}`}
        disabled={duration <= 0}
        onChange={setScrub}
        onCommit={(value) => {
          onSeek(value);
          setTime(value);
          setScrub(null);
        }}
      />
      <span className="w-14 text-right text-xs tabular-nums text-white/80">
        {formatTime(duration)}
      </span>
    </div>
  );
});

PlayerTimeline.displayName = 'PlayerTimeline';
