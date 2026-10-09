import { useCallback } from 'react';
import { open } from '@tauri-apps/plugin-shell';
import { useUIStore } from '../stores/uiStore';

/** Opens a URL in the system browser and reports a failure instead of swallowing it. */
export function useOpenExternal() {
  const showToast = useUIStore((s) => s.showToast);

  return useCallback(
    async (url: string) => {
      try {
        await open(url);
      } catch (err) {
        console.error('[openExternal] failed to open', url, err);
        showToast({
          type: 'error',
          title: 'Could not open the link',
          message: url,
        });
      }
    },
    [showToast]
  );
}
