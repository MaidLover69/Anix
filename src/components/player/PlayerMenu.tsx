import React, { useEffect, useRef, useState } from 'react';
import { Check } from 'lucide-react';
import { IconButton } from './IconButton';

export interface PlayerMenuOption {
  value: string;
  label: string;
}

interface PlayerMenuProps {
  /** Accessible name and tooltip of the trigger. */
  label: string;
  /** Trigger content: an icon or a short value such as "1.5×". */
  children: React.ReactNode;
  value: string;
  options: PlayerMenuOption[];
  onChange: (value: string) => void;
  /** Lets the player keep its controls on screen while the menu is open. */
  onOpenChange?: (open: boolean) => void;
}

/** A ghost button in the control bar that opens a single-choice menu above itself. */
export const PlayerMenu: React.FC<PlayerMenuProps> = ({
  label,
  children,
  value,
  options,
  onChange,
  onOpenChange,
}) => {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const onOpenChangeRef = useRef(onOpenChange);
  onOpenChangeRef.current = onOpenChange;

  useEffect(() => {
    if (!open) return;
    onOpenChangeRef.current?.(true);

    const list = listRef.current;
    (list?.querySelector<HTMLElement>('[aria-checked="true"]') ?? list?.querySelector<HTMLElement>('button'))?.focus();

    const onPointerDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onPointerDown);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      onOpenChangeRef.current?.(false);
    };
  }, [open]);

  const close = (returnFocus: boolean) => {
    setOpen(false);
    if (returnFocus) triggerRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    // Keys typed in the menu belong to the menu, not to the player shortcuts.
    e.stopPropagation();
    const items = Array.from(listRef.current?.querySelectorAll<HTMLElement>('button') ?? []);
    const index = items.indexOf(document.activeElement as HTMLElement);
    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        close(true);
        break;
      case 'ArrowDown':
        e.preventDefault();
        items[(index + 1) % items.length]?.focus();
        break;
      case 'ArrowUp':
        e.preventDefault();
        items[(index - 1 + items.length) % items.length]?.focus();
        break;
      case 'Home':
        e.preventDefault();
        items[0]?.focus();
        break;
      case 'End':
        e.preventDefault();
        items[items.length - 1]?.focus();
        break;
      case 'Tab':
        close(false);
        break;
    }
  };

  return (
    <div ref={rootRef} className="relative">
      <IconButton
        ref={triggerRef}
        label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        active={open}
        onClick={() => setOpen((prev) => !prev)}
      >
        {children}
      </IconButton>

      {open && (
        <div
          ref={listRef}
          role="menu"
          aria-label={label}
          onKeyDown={handleKeyDown}
          className="absolute bottom-full right-0 z-40 mb-2 max-h-80 min-w-[9rem] overflow-y-auto rounded-lg border border-border-strong bg-bg-elevated p-1 shadow-[0_8px_24px_rgba(0,0,0,0.5)]"
        >
          {options.map((option) => {
            const selected = option.value === value;
            return (
              <button
                key={option.value}
                type="button"
                role="menuitemradio"
                aria-checked={selected}
                onClick={() => {
                  onChange(option.value);
                  close(false);
                }}
                className={`flex h-8 w-full items-center justify-between gap-3 rounded-md px-3 text-left text-[13px] whitespace-nowrap cursor-pointer transition-colors ${
                  selected
                    ? 'bg-accent-tint text-accent-300'
                    : 'text-text-secondary hover:bg-white/10 hover:text-text-primary'
                }`}
              >
                <span>{option.label}</span>
                {selected && <Check aria-hidden="true" className="h-3.5 w-3.5 flex-shrink-0" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
};
