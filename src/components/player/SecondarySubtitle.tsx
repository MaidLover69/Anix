import React, { useEffect, useState } from 'react';
import type { RefObject } from 'react';

interface SecondarySubtitleProps {
  videoRef: RefObject<HTMLVideoElement>;
  /** WebVTT URL of the second track. */
  src: string;
  fontSize: number;
  color: string;
  background: string;
  /** The same delay the main subtitles use. */
  delayMs: number;
  onError: () => void;
}

const TICK_MS = 100;

function cueText(cue: TextTrackCue): string {
  const text = (cue as VTTCue).text ?? '';
  return text.replace(/<[^>]+>/g, '').trim();
}

/**
 * A second subtitle track drawn at the top of the picture. It is loaded as a `metadata`
 * track, which the webview never renders itself and which hls.js and the main subtitle
 * selection leave alone; its cues are drawn here.
 */
export const SecondarySubtitle: React.FC<SecondarySubtitleProps> = ({
  videoRef,
  src,
  fontSize,
  color,
  background,
  delayMs,
  onError,
}) => {
  const [lines, setLines] = useState<string[]>([]);
  const [top, setTop] = useState(0);
  const onErrorRef = React.useRef(onError);
  onErrorRef.current = onError;

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const element = document.createElement('track');
    element.kind = 'metadata';
    element.src = src;
    const handleError = () => onErrorRef.current();
    element.addEventListener('error', handleError);
    video.appendChild(element);
    element.track.mode = 'hidden';

    const offset = delayMs / 1000;
    let shown = '';
    const update = () => {
      const cues = element.track.cues;
      if (!cues) return;
      const time = video.currentTime - offset;
      const active: string[] = [];
      for (let i = 0; i < cues.length; i++) {
        const cue = cues[i];
        if (cue.startTime <= time && time < cue.endTime) {
          const text = cueText(cue);
          if (text) active.push(text);
        }
      }
      const key = active.join('\n');
      if (key !== shown) {
        shown = key;
        setLines(active);
      }
    };
    const interval = window.setInterval(update, TICK_MS);
    video.addEventListener('seeked', update);
    element.addEventListener('load', update);

    return () => {
      window.clearInterval(interval);
      video.removeEventListener('seeked', update);
      element.removeEventListener('load', update);
      element.removeEventListener('error', handleError);
      element.remove();
      setLines([]);
    };
  }, [videoRef, src, delayMs]);

  // The picture is letterboxed inside the element; the text sits at the top of the picture.
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const measure = () => {
      const { clientWidth: w, clientHeight: h, videoWidth: vw, videoHeight: vh } = video;
      setTop(vw > 0 && vh > 0 ? Math.max(0, (h - (w * vh) / vw) / 2) : 0);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(video);
    video.addEventListener('loadedmetadata', measure);
    video.addEventListener('resize', measure);
    return () => {
      observer.disconnect();
      video.removeEventListener('loadedmetadata', measure);
      video.removeEventListener('resize', measure);
    };
  }, [videoRef]);

  if (lines.length === 0) return null;
  return (
    <div
      aria-live="off"
      className="pointer-events-none absolute inset-x-0 z-10 flex flex-col items-center gap-1 px-[8%]"
      style={{ top: top + 16 }}
    >
      {lines.map((line, index) => (
        <span
          key={index}
          className="whitespace-pre-line text-center leading-snug"
          style={{
            fontSize: `${fontSize}px`,
            color,
            backgroundColor: background,
            padding: '1px 8px',
            textShadow: '0 0 4px #000, 0 0 8px #000',
          }}
        >
          {line}
        </span>
      ))}
    </div>
  );
};
