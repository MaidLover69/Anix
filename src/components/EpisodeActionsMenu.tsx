import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import clsx from 'clsx';

export interface MenuAction {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
}

interface EpisodeActionsMenuProps {
  /** Where the menu opens: the pointer of a right-click, or the corner of the button. */
  at: { x: number; y: number };
  actions: MenuAction[];
  label: string;
  onClose: () => void;
  /** Gets focus back when the menu closes from the keyboard. */
  returnFocusTo?: HTMLElement | null;
}

const MARGIN = 8;

/**
 * A small action menu (role="menu") at a point on screen. Arrow keys move, Enter picks,
 * Escape or a click elsewhere closes it, and Tab closes it and moves on: it never holds focus.
 */
export const EpisodeActionsMenu: React.FC<EpisodeActionsMenuProps> = ({
  at,
  actions,
  label,
  onClose,
  returnFocusTo,
}) => {
  const menuRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [position, setPosition] = useState(at);

  // Keep the whole menu on screen.
  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const { width, height } = menu.getBoundingClientRect();
    setPosition({
      x: Math.max(MARGIN, Math.min(at.x, window.innerWidth - width - MARGIN)),
      y: Math.max(MARGIN, Math.min(at.y, window.innerHeight - height - MARGIN)),
    });
  }, [at]);

  useEffect(() => {
    itemRefs.current.find((item) => item && !item.disabled)?.focus();
  }, []);

  useEffect(() => {
    const close = (event: Event) => {
      if (event.target instanceof Node && menuRef.current?.contains(event.target)) return;
      onClose();
    };
    window.addEventListener('pointerdown', close, true);
    window.addEventListener('scroll', close, true);
    window.addEventListener('blur', onClose);
    return () => {
      window.removeEventListener('pointerdown', close, true);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('blur', onClose);
    };
  }, [onClose]);

  const closeAndReturn = () => {
    onClose();
    returnFocusTo?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    const enabled = itemRefs.current.filter((item): item is HTMLButtonElement => !!item && !item.disabled);
    const index = enabled.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeAndReturn();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const step = e.key === 'ArrowDown' ? 1 : -1;
      enabled[(index + step + enabled.length) % enabled.length]?.focus();
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      (e.key === 'Home' ? enabled[0] : enabled[enabled.length - 1])?.focus();
    } else if (e.key === 'Tab') {
      // Focus moves on from the trigger, as if the menu had not been there.
      returnFocusTo?.focus();
      onClose();
    }
  };

  return createPortal(
    <div
      ref={menuRef}
      role="menu"
      aria-label={label}
      onKeyDown={handleKeyDown}
      onContextMenu={(e) => e.preventDefault()}
      style={{ left: position.x, top: position.y }}
      className="ui-menu ui-pop-in fixed z-[80] min-w-[15rem] rounded-xl border border-border-strong bg-bg-elevated p-1 shadow-pop"
    >
      {actions.map((action, i) => (
        <button
          key={action.label}
          ref={(el) => {
            itemRefs.current[i] = el;
          }}
          type="button"
          role="menuitem"
          tabIndex={-1}
          disabled={action.disabled}
          onClick={() => {
            action.onSelect();
            closeAndReturn();
          }}
          className={clsx(
            'flex h-8 w-full cursor-pointer items-center rounded-md px-3 text-left text-[13px] text-text-primary transition-colors',
            'hover:bg-ink/10 focus:bg-ink/10 focus-visible:outline-none disabled:pointer-events-none disabled:opacity-40'
          )}
        >
          {action.label}
        </button>
      ))}
    </div>,
    document.body
  );
};
