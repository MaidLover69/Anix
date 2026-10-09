import React, { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { X, CheckCircle, AlertTriangle, AlertCircle, Info } from 'lucide-react';
import clsx from 'clsx';
import { useUIStore } from '../stores/uiStore';
import type { ToastMessage } from '../stores/uiStore';
import { Button } from './ui/Button';

interface ToastContainerProps {
  /**
   * `bottom-right` for ordinary screens; `top-center` on the player, where the bottom
   * edge holds the seek bar and controls.
   */
  placement?: 'bottom-right' | 'top-center';
}

const ICONS: Record<ToastMessage['type'], React.ReactNode> = {
  success: <CheckCircle aria-hidden="true" className="mt-0.5 h-4 w-4 flex-shrink-0 text-state-success" />,
  warning: <AlertTriangle aria-hidden="true" className="mt-0.5 h-4 w-4 flex-shrink-0 text-state-warning" />,
  error: <AlertCircle aria-hidden="true" className="mt-0.5 h-4 w-4 flex-shrink-0 text-state-error" />,
  info: <Info aria-hidden="true" className="mt-0.5 h-4 w-4 flex-shrink-0 text-accent-300" />,
};

export const ToastContainer: React.FC<ToastContainerProps> = ({ placement = 'bottom-right' }) => {
  const toasts = useUIStore((s) => s.toasts);
  const removeToast = useUIStore((s) => s.removeToast);

  // Only the fullscreen element is painted while fullscreen, so toasts follow it.
  const [fullscreenElement, setFullscreenElement] = useState<Element | null>(
    () => document.fullscreenElement
  );
  useEffect(() => {
    const sync = () => setFullscreenElement(document.fullscreenElement);
    document.addEventListener('fullscreenchange', sync);
    return () => document.removeEventListener('fullscreenchange', sync);
  }, []);

  // The live region stays mounted while empty so that additions are announced.
  return createPortal(
    <div
      aria-live="polite"
      aria-relevant="additions"
      className={clsx(
        'pointer-events-none fixed z-[80] flex w-full flex-col gap-2 px-4',
        placement === 'top-center'
          ? 'left-1/2 top-24 max-w-[480px] -translate-x-1/2 items-center'
          : 'bottom-6 right-2 max-w-[392px] items-stretch'
      )}
    >
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role={toast.type === 'error' ? 'alert' : 'status'}
          className="pointer-events-auto flex max-w-full items-start gap-3 rounded-xl border border-border-strong bg-bg-elevated py-3 pl-4 pr-2 shadow-[0_8px_24px_rgba(0,0,0,0.5)] animate-slide-up"
        >
          {ICONS[toast.type]}
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="break-words text-[13px] font-medium leading-5 text-text-primary select-text">
              {toast.title}
            </span>
            {toast.message && (
              <p className="mt-0.5 break-words text-xs leading-[18px] text-text-secondary select-text">
                {toast.message}
              </p>
            )}
          </div>
          <Button
            icon
            variant="ghost"
            size="sm"
            aria-label="Dismiss notification"
            onClick={() => removeToast(toast.id)}
            className="-my-1"
          >
            <X aria-hidden="true" className="h-3.5 w-3.5" />
          </Button>
        </div>
      ))}
    </div>,
    fullscreenElement ?? document.body
  );
};
