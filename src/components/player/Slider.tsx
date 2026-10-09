import React, { useRef, useState } from 'react';

interface SliderProps {
  value: number;
  min?: number;
  max: number;
  /** Values snap to multiples of this when set. */
  step?: number;
  /** Change per arrow key press; defaults to `step` or 1% of the range. */
  keyStep?: number;
  /** Shown as a lighter fill behind the value (same unit as `value`). */
  buffered?: number;
  /** Accessible name. */
  label: string;
  valueText?: string;
  /** Called continuously while dragging and on every key press. */
  onChange: (value: number) => void;
  /** Called once when a drag ends or a key press is done. */
  onCommit?: (value: number) => void;
  disabled?: boolean;
  className?: string;
}

/**
 * A slider with a fixed-height track, an accent fill and a thumb that grows through a
 * transform, so hovering never moves the layout. A mouse press does not take focus, which
 * keeps the arrow keys with the player shortcuts.
 */
export const Slider: React.FC<SliderProps> = ({
  value,
  min = 0,
  max,
  step,
  keyStep,
  buffered,
  label,
  valueText,
  onChange,
  onCommit,
  disabled = false,
  className = '',
}) => {
  const trackRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);
  const lastValueRef = useRef(value);
  const [dragging, setDragging] = useState(false);

  const span = max - min;
  const usable = !disabled && span > 0;
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const snap = (v: number) => (step ? min + Math.round((v - min) / step) * step : v);
  const fraction = (v: number) => (span > 0 ? (clamp(v) - min) / span : 0);

  const valueAt = (clientX: number) => {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0) return clamp(value);
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    return clamp(snap(min + ratio * span));
  };

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!usable || e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    draggingRef.current = true;
    setDragging(true);
    lastValueRef.current = valueAt(e.clientX);
    onChange(lastValueRef.current);
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    lastValueRef.current = valueAt(e.clientX);
    onChange(lastValueRef.current);
  };

  const endDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    draggingRef.current = false;
    setDragging(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    onCommit?.(lastValueRef.current);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!usable || e.ctrlKey || e.metaKey || e.altKey) return;
    const delta = keyStep ?? step ?? span / 100;
    let next: number;
    switch (e.key) {
      case 'ArrowRight':
      case 'ArrowUp':
        next = value + delta;
        break;
      case 'ArrowLeft':
      case 'ArrowDown':
        next = value - delta;
        break;
      case 'Home':
        next = min;
        break;
      case 'End':
        next = max;
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
    next = clamp(snap(next));
    onChange(next);
    onCommit?.(next);
  };

  const filled = `${fraction(value) * 100}%`;

  return (
    <div
      role="slider"
      tabIndex={usable ? 0 : -1}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={clamp(value)}
      aria-valuetext={valueText}
      aria-disabled={!usable}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onKeyDown={handleKeyDown}
      className={`group/slider relative flex h-4 items-center touch-none rounded-sm ${
        usable ? 'cursor-pointer' : 'opacity-50'
      } ${className}`}
    >
      <div ref={trackRef} className="relative h-1 w-full rounded-full bg-white/20">
        {buffered !== undefined && (
          <div
            className="absolute inset-y-0 left-0 rounded-full bg-white/25"
            style={{ width: `${fraction(buffered) * 100}%` }}
          />
        )}
        <div className="absolute inset-y-0 left-0 rounded-full bg-accent-500" style={{ width: filled }} />
        <div
          className={`absolute top-1/2 -ml-1.5 -mt-1.5 h-3 w-3 rounded-full bg-white shadow transition-transform duration-100 group-hover/slider:scale-125 group-focus-visible/slider:scale-125 ${
            dragging ? 'scale-125' : 'scale-75'
          }`}
          style={{ left: filled }}
        />
      </div>
    </div>
  );
};
