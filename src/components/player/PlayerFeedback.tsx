import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react';

export interface FeedbackMessage {
  text: string;
  /** Seek feedback sits at the side it seeks towards; everything else at the top. */
  side?: 'left' | 'right';
  tone?: 'info' | 'error';
}

export interface PlayerFeedbackHandle {
  show: (message: FeedbackMessage) => void;
}

/**
 * Short-lived messages drawn inside the player, so they are also visible in fullscreen
 * (the app's toasts live outside the fullscreen element). Driven through a ref so that a
 * burst of messages, such as held arrow keys, re-renders nothing but this element.
 */
export const PlayerFeedback = forwardRef<PlayerFeedbackHandle>((_props, ref) => {
  const [message, setMessage] = useState<FeedbackMessage | null>(null);
  const timerRef = useRef<number | null>(null);

  useImperativeHandle(
    ref,
    () => ({
      show: (next) => {
        if (timerRef.current !== null) window.clearTimeout(timerRef.current);
        setMessage(next);
        const duration = next.side ? 700 : next.tone === 'error' ? 6000 : 2500;
        timerRef.current = window.setTimeout(() => {
          timerRef.current = null;
          setMessage(null);
        }, duration);
      },
    }),
    []
  );

  useEffect(
    () => () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    },
    []
  );

  if (!message) return null;

  const position =
    message.side === 'left'
      ? 'left-[12%] top-1/2 -translate-y-1/2'
      : message.side === 'right'
      ? 'right-[12%] top-1/2 -translate-y-1/2'
      : 'left-1/2 top-16 -translate-x-1/2';

  return (
    <div
      role={message.tone === 'error' ? 'alert' : 'status'}
      className={`pointer-events-none absolute z-30 max-w-[80%] rounded-md bg-black/75 px-3 py-1.5 text-center text-[13px] font-medium tabular-nums ${
        message.tone === 'error' ? 'text-state-error' : 'text-white'
      } ${position}`}
    >
      {message.text}
    </div>
  );
});

PlayerFeedback.displayName = 'PlayerFeedback';
