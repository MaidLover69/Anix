import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { DesktopAppearance } from '../types';

export const DEFAULT_DESKTOP_APPEARANCE: DesktopAppearance = {
  button_layout: { left: [], right: ['minimize', 'maximize', 'close'] },
  button_layout_source: 'default',
  accent_color: null,
  accent_source: null,
};

/**
 * The desktop's window-button layout and accent colour. Read again when the window regains
 * focus, so a change made in the desktop settings shows up without a restart.
 */
export function useDesktopAppearance() {
  return useQuery<DesktopAppearance>({
    queryKey: ['desktopAppearance'],
    queryFn: () => invoke<DesktopAppearance>('get_desktop_appearance'),
    staleTime: 60 * 1000,
    refetchOnWindowFocus: true,
    retry: false,
  });
}
