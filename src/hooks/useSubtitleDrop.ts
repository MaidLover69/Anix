import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { LocalSubtitle } from './useSubtitleTracks';

const SUBTITLE_FILE = /\.(srt|ass|ssa|vtt)$/i;

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

export function isSubtitlePath(path: string): boolean {
  return SUBTITLE_FILE.test(path);
}

/**
 * Subtitle files dropped on the window while the player is open. Each one is converted to
 * WebVTT by the backend and kept as a blob URL for as long as the player is mounted.
 */
export function useSubtitleDrop(onAdded: (file: LocalSubtitle, index: number) => void, onFailed: (message: string) => void) {
  const [files, setFiles] = useState<LocalSubtitle[]>([]);
  const [dragging, setDragging] = useState(false);
  const filesRef = useRef(files);
  filesRef.current = files;
  const callbacks = useRef({ onAdded, onFailed });
  callbacks.current = { onAdded, onFailed };

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    let count = 0;

    const add = async (path: string) => {
      try {
        const vtt = await invoke<string>('read_subtitle_file', { path });
        if (disposed) return;
        const file: LocalSubtitle = {
          label: fileName(path),
          url: URL.createObjectURL(new Blob([vtt], { type: 'text/vtt' })),
        };
        const index = count++;
        setFiles((prev) => [...prev, file]);
        callbacks.current.onAdded(file, index);
      } catch (err) {
        if (!disposed) callbacks.current.onFailed(typeof err === 'string' ? err : 'That file could not be read');
      }
    };

    getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === 'enter') setDragging(payload.paths.some(isSubtitlePath));
        else if (payload.type === 'leave') setDragging(false);
        else if (payload.type === 'drop') {
          setDragging(false);
          const subtitles = payload.paths.filter(isSubtitlePath);
          if (subtitles.length === 0) {
            if (payload.paths.length > 0) callbacks.current.onFailed('Drop a .srt, .ass or .vtt file to use it as subtitles');
            return;
          }
          // One at a time keeps the order of the new tracks the order of the files.
          subtitles.reduce((chain, path) => chain.then(() => add(path)), Promise.resolve());
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unlisten?.();
      filesRef.current.forEach((file) => URL.revokeObjectURL(file.url));
    };
  }, []);

  return { files, dragging };
}
