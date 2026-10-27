import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useUIStore } from '../stores/uiStore';
import { t } from '../i18n';

/**
 * Opens a URL in the system browser and reports a failure instead of swallowing it. The
 * backend starts the browser without the AppImage's library paths (`open_external`).
 */
export function useOpenExternal() {
  const showToast = useUIStore((s) => s.showToast);

  return useCallback(
    async (url: string) => {
      try {
        await invoke('open_external', { url });
      } catch (err) {
        console.error('[openExternal] failed to open', url, err);
        showToast({
          type: 'error',
          title: t('useOpenExternal.failed'),
          message: url,
        });
      }
    },
    [showToast]
  );
}
