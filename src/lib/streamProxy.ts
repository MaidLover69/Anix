import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { StreamItem, SubtitleItem } from '../types';

// The webview cannot set Referer/Origin on media requests, and its own origin differs
// between `tauri dev` and a packaged build. Streams and subtitles are therefore fetched
// through a loopback proxy in the Rust backend, which adds the headers the CDN expects.

type ProxiedStream = Pick<StreamItem, 'url' | 'referer' | 'origin' | 'headers'>;

function encodeHeaders(headers: Record<string, string>): string {
  const bytes = new TextEncoder().encode(JSON.stringify(headers));
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function withStreamParams(params: URLSearchParams, stream?: Partial<ProxiedStream> | null) {
  if (stream?.referer) params.set('referer', stream.referer);
  if (stream?.origin) params.set('origin', stream.origin);
  if (stream?.headers && Object.keys(stream.headers).length > 0) {
    params.set('h', encodeHeaders(stream.headers));
  }
}

/** URL that plays `stream` through the backend proxy (HLS playlists are rewritten recursively). */
export function buildProxyUrl(base: string, stream: ProxiedStream): string {
  const params = new URLSearchParams({ url: stream.url });
  withStreamParams(params, stream);
  return `${base}/proxy?${params.toString()}`;
}

/** URL that serves `subtitle` as WebVTT (SRT and ASS are converted by the backend). */
export function buildSubtitleUrl(
  base: string,
  subtitle: Pick<SubtitleItem, 'url'>,
  stream?: Partial<ProxiedStream> | null
): string {
  const params = new URLSearchParams({ url: subtitle.url });
  withStreamParams(params, stream);
  return `${base}/subtitle?${params.toString()}`;
}

/** Base URL of the backend stream proxy, e.g. `http://127.0.0.1:41873/<token>`. */
export function useStreamProxyBase() {
  return useQuery<string>({
    queryKey: ['streamProxyBase'],
    queryFn: () => invoke<string>('get_stream_proxy_base'),
    staleTime: Infinity,
    gcTime: Infinity,
    retry: 1,
  });
}
