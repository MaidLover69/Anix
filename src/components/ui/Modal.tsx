import React, { useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './Button';
import { t } from '../../i18n';

export type ModalSize = 'sm' | 'md' | 'lg' | 'xl' | '2xl';

export interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  /** Names the dialog for assistive tech; shown in the header unless `hideHeader`. */
  title: React.ReactNode;
  children: React.ReactNode;
  /** Panel width: sm 384px, md 448px, lg 560px (default), xl 672px, 2xl 896px. */
  size?: ModalSize;
  /** Extra content under the title (a caption, a toggle). */
  header?: React.ReactNode;
  /** Id of the element that describes the dialog (`aria-describedby`). */
  describedBy?: string;
  /** Action row; its children are right-aligned. */
  footer?: React.ReactNode;
  /** Drops the title row and close button; the title is still read out. */
  hideHeader?: boolean;
  /** `false` removes the panel padding, for edge-to-edge content. */
  padded?: boolean;
  position?: 'center' | 'top';
  /**
   * Portal target. Defaults to the fullscreen element when there is one (nothing outside
   * it is painted), otherwise `document.body`.
   */
  container?: HTMLElement | null;
  className?: string;
}

const SIZE_CLASSES: Record<ModalSize, string> = {
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-[560px]',
  xl: 'max-w-2xl',
  '2xl': 'max-w-4xl',
};

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), iframe, audio[controls], video[controls], [tabindex]:not([tabindex="-1"])';

// Open modals, oldest first. Only the last one reacts to Escape and Tab.
const openModals: symbol[] = [];

/** True while any `Modal` is open. Global hotkey handlers use it to stand down. */
export function hasOpenModal(): boolean {
  return openModals.length > 0;
}

export const Modal: React.FC<ModalProps> = ({
  isOpen,
  onClose,
  title,
  children,
  size = 'lg',
  header,
  describedBy,
  footer,
  hideHeader = false,
  padded = true,
  position = 'center',
  container,
  className,
}) => {
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const pressStartedOnBackdrop = useRef(false);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const [fullscreenElement, setFullscreenElement] = useState<Element | null>(
    () => document.fullscreenElement
  );
  useEffect(() => {
    const sync = () => setFullscreenElement(document.fullscreenElement);
    document.addEventListener('fullscreenchange', sync);
    return () => document.removeEventListener('fullscreenchange', sync);
  }, []);

  useEffect(() => {
    if (!isOpen) return;

    const token = Symbol('modal');
    openModals.push(token);

    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    const panel = panelRef.current;
    if (panel) {
      (panel.querySelector<HTMLElement>('[data-autofocus]') ?? panel).focus();
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (openModals[openModals.length - 1] !== token) return;

      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onCloseRef.current();
        return;
      }

      if (e.key !== 'Tab' || !panelRef.current) return;
      const root = panelRef.current;
      const focusable = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => el.getClientRects().length > 0
      );
      if (focusable.length === 0) {
        e.preventDefault();
        root.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (!root.contains(active) || active === root) {
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

    // Bubble phase on purpose: a child that handles Escape itself (an open
    // dropdown) stops propagation and the dialog stays open.
    document.addEventListener('keydown', handleKeyDown);

    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      const index = openModals.lastIndexOf(token);
      if (index !== -1) openModals.splice(index, 1);
      document.body.style.overflow = previousOverflow;
      if (trigger && document.contains(trigger)) trigger.focus();
    };
  }, [isOpen]);

  if (!isOpen) return null;

  return createPortal(
    <div
      className="ui-backdrop fixed inset-0 z-[60] flex overflow-y-auto bg-black/60 px-4 pb-4 pt-12 animate-fade-in"
      onMouseDown={(e) => {
        pressStartedOnBackdrop.current = e.target === e.currentTarget;
      }}
      onClick={(e) => {
        if (pressStartedOnBackdrop.current && e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={describedBy}
        tabIndex={-1}
        className={clsx(
          'ui-modal relative flex w-full flex-col rounded-2xl border border-border bg-bg-elevated shadow-pop outline-none animate-scale-up',
          position === 'top' ? 'mx-auto mb-auto mt-12' : 'm-auto',
          SIZE_CLASSES[size],
          padded && 'p-6',
          className
        )}
      >
        {hideHeader ? (
          <h2 id={titleId} className="sr-only">
            {title}
          </h2>
        ) : (
          <div className="mb-4 flex items-start justify-between gap-4">
            <div className="min-w-0 flex-1">
              <h2 id={titleId} className="truncate text-base font-semibold text-text-primary select-text">
                {title}
              </h2>
              {header}
            </div>
            <Button
              icon
              variant="ghost"
              size="sm"
              aria-label={t('common.close')}
              onClick={onClose}
              className="-me-2 -mt-1"
            >
              <X aria-hidden="true" className="h-4 w-4" />
            </Button>
          </div>
        )}

        {children}

        {footer && <div className="mt-6 flex items-center justify-end gap-2">{footer}</div>}
      </div>
    </div>,
    container ?? fullscreenElement ?? document.body
  );
};
