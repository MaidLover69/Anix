import React, { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from './ui/Button';
import { Modal } from './ui/Modal';
import { useConfirmCountdown, type Answer } from './player/EngineConfirmDialog';
import { useFastDrawingStatus } from '../hooks/useSystem';
import { t } from '../i18n';

/**
 * After fast drawing was turned on (Settings › System, NVIDIA only) or WebKit changed: asks
 * whether the window looks right. The page keeps running when the window draws blank, so no
 * answer in time turns fast drawing off again and restarts.
 */
export const FastDrawingConfirmDialog: React.FC = () => {
  const status = useFastDrawingStatus().data;
  const [answered, setAnswered] = useState(false);
  const open = status?.ask === true && !answered;

  const answer = useCallback((kind: Answer) => {
    setAnswered(true);
    invoke('fast_drawing_answer', { answer: kind }).catch((e) => console.warn('fast drawing answer:', e));
  }, []);
  const timeout = useCallback(() => answer('no_answer'), [answer]);
  const left = useConfirmCountdown(open, timeout);

  return (
    <Modal
      isOpen={open}
      // Whoever can close this dialog can see the window.
      onClose={() => answer('keep')}
      title={t('fastDrawing.dialogTitle')}
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
      <p className="text-[13px] text-text-secondary">{t('fastDrawing.dialogBody')}</p>
      <p className="mt-3 text-[13px] font-medium tabular-nums text-text-primary">
        {t('engineConfirmDialog.switchingBackIn', { count: left })}
      </p>
    </Modal>
  );
};
