import { invoke } from '@tauri-apps/api/core';

/** Covers and banners are also saved on disk (Settings › System sets the limit), so they still
 *  show when the picture host cannot be reached. One address is asked once per run, one at a time. */
const asked = new Set<string>();
let queue: Promise<unknown> = Promise.resolve();

/** Saves a picture that has just loaded. Never throws; a failure only means it is not kept. */
export function rememberImage(url: string): void {
  if (!url.startsWith('https://') || asked.has(url)) return;
  asked.add(url);
  queue = queue.then(() => invoke('save_image', { url })).catch(() => undefined);
}

/** The saved picture as a data address, or null when there is none (or no app to ask). */
export async function savedImage(url: string): Promise<string | null> {
  if (!url.startsWith('https://')) return null;
  try {
    return await invoke<string | null>('saved_image', { url });
  } catch {
    return null;
  }
}
