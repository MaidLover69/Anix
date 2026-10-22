import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '../ui/Button';
import { Modal } from '../ui/Modal';
import { MPV_STATUS_KEY, useEmbeddedMpvStatus } from '../../hooks/useEmbeddedMpv';
import { t } from '../../i18n';

/** Seconds the question waits before Anix assumes the window is blank and switches back. */
export const CONFIRM_SECONDS = 15;

type Answer = 'keep' | 'switch_back' | 'no_answer';

/**
 * After a switch to embedded mpv (or an update of libmpv or WebKit), asks whether the window
 * shows correctly. A blank window cannot be detected from the page, so no answer in time goes
 * back to the web player and restarts on the same screen. Also reopens that screen after a
 * restart Anix asked for.
 */
export const EngineConfirmDialog: React.FC = () => {
  const status = useEmbeddedMpvStatus().data;
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const location = useLocation();
  const routeRef = useRef('');
  routeRef.current = `${location.pathname}${location.search}`;
  const [answered, setAnswered] = useState(false);
  const [left, setLeft] = useState(CONFIRM_SECONDS);
  const open = status?.ask_confirm === true && !answered;

  useEffect(() => {
    invoke<string | null>('embedded_mpv_take_route')
      .then((route) => {
        if (route && route.startsWith('/') && !route.startsWith('//')) navigate(route, { replace: true });
      })
      .catch(() => {});
  }, [navigate]);

  const answer = useCallback(
    (kind: Answer) => {
      setAnswered(true);
      invoke('embedded_mpv_answer', { answer: kind, route: routeRef.current })
        .catch((e) => console.warn('embedded mpv answer:', e))
        .finally(() => queryClient.invalidateQueries({ queryKey: MPV_STATUS_KEY }));
    },
    [queryClient]
  );

  useEffect(() => {
    if (!open) return;
    // Only counts while the window can be seen (not hidden to the tray).
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') setLeft((s) => Math.max(0, s - 1));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [open]);
  useEffect(() => {
    if (open && left === 0) answer('no_answer');
  }, [open, left, answer]);

  return (
    <Modal
      isOpen={open}
      // Whoever can close this dialog can see the window.
      onClose={() => answer('keep')}
      title={t('engineConfirmDialog.title')}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={() => answer('switch_back')}>
            {t('engineConfirmDialog.switchBack')}
          </Button>
          <Button variant="primary" onClick={() => answer('keep')}>
            {t('engineConfirmDialog.keep')}
          </Button>
        </>
      }
    >
      <p className="text-[13px] text-text-secondary">
        {t('engineConfirmDialog.body')}
      </p>
      <p className="mt-3 text-[13px] font-medium tabular-nums text-text-primary">
        {t('engineConfirmDialog.switchingBackIn', { count: left })}
      </p>
    </Modal>
  );
};
