import React, { useEffect, useId, useRef } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { Button } from '../ui/Button';
import { t } from '../../i18n';

interface PlayerSidePanelProps {
  isOpen: boolean;
  onClose: () => void;
  title: React.ReactNode;
  /** One muted line under the title. */
  subtitle?: string;
  children: React.ReactNode;
  /** Portal target; the player passes the element it shows fullscreen. */
  container?: HTMLElement | null;
}

/**
 * The one roomy side panel of the player, shared by Settings, Subtitles and Episodes: full height
 * on the right, edged on the picture side. The backdrop only lightly dims the picture.
 */
export const SIDE_PANEL_CLASS =
  'flex h-full w-full max-w-[26rem] flex-col border-l border-border bg-bg-elevated shadow-pop outline-none animate-slide-left motion-reduce:animate-none';
export const SIDE_PANEL_BACKDROP_CLASS =
  'absolute inset-0 z-[60] flex justify-end bg-black/30 animate-fade-in motion-reduce:animate-none';

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

const Panel: React.FC<Omit<PlayerSidePanelProps, 'isOpen' | 'container'>> = ({ onClose, title, subtitle, children }) => {
  const panelRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  // Focus moves into the panel, stays inside it, and returns to the button that opened it.
  useEffect(() => {
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = panelRef.current;
    (panel?.querySelector<HTMLElement>('[aria-checked="true"], [aria-pressed="true"]') ?? panel)?.focus({
      preventScroll: true,
    });

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onCloseRef.current();
        return;
      }
      if (e.key !== 'Tab' || !panel) return;
      const focusable = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (!panel.contains(active) || active === panel) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
      } else if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      if (trigger && document.contains(trigger)) trigger.focus();
    };
  }, []);

  return (
    <div
      ref={panelRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      tabIndex={-1}
      // Panel text reads in the page direction; only the player's bar and seek zones stay left to right.
      dir={document.documentElement.dir || 'ltr'}
      onClick={(e) => e.stopPropagation()}
      className={SIDE_PANEL_CLASS}
    >
      <div className="flex items-center justify-between gap-3 px-5 pb-3 pt-5">
        <div className="min-w-0">
          <h2 id={titleId} className="text-lg font-extrabold text-text-primary">
            {title}
          </h2>
          {subtitle && <p className="truncate text-xs text-text-muted">{subtitle}</p>}
        </div>
        <Button icon size="sm" variant="ghost" aria-label={t('common.close')} title={t('playerSidePanel.closeWithKey')} onClick={onClose}>
          <X aria-hidden="true" className="h-4 w-4" />
        </Button>
      </div>
      <div className="flex flex-1 flex-col gap-6 overflow-y-auto px-5 pb-5 pt-1">{children}</div>
    </div>
  );
};

/** The full-height "Now playing" panel on the right of the player. */
export const PlayerSidePanel: React.FC<PlayerSidePanelProps> = ({ isOpen, container, ...panelProps }) => {
  if (!isOpen) return null;
  return createPortal(
    // Left to right like the player, so the panel sits on its right in every language.
    <div
      dir="ltr"
      className={SIDE_PANEL_BACKDROP_CLASS}
      onClick={panelProps.onClose}
    >
      <Panel {...panelProps} />
    </div>,
    container ?? document.body
  );
};

export const PanelSection: React.FC<{ title: React.ReactNode; children: React.ReactNode }> = ({ title, children }) => {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-2">
      <h3 id={id} className="text-[11px] font-extrabold uppercase tracking-widest text-text-muted">
        {title}
      </h3>
      {children}
    </section>
  );
};
