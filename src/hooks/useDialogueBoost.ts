import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';

interface AudioGraph {
  context: AudioContext;
  source: MediaElementAudioSourceNode;
  compressor: DynamicsCompressorNode;
  gain: GainNode;
}

type AudioContextClass = typeof AudioContext;

function audioContextClass(): AudioContextClass | null {
  const w = window as unknown as { AudioContext?: AudioContextClass; webkitAudioContext?: AudioContextClass };
  return w.AudioContext ?? w.webkitAudioContext ?? null;
}

/**
 * Dialogue boost ("night mode"): the element's audio goes through a compressor that evens out
 * loud effects and quiet speech, plus a little make-up gain.
 *
 * An element can be connected to Web Audio only once and then always plays through it, so the
 * graph is built the first time the boost is switched on and is bypassed when it is off. It is
 * only connected while its context is running; a suspended context would silence the video.
 * The stream proxy answers with `Access-Control-Allow-Origin: *` and the element is
 * `crossOrigin="anonymous"`, so the audio is not withheld as cross-origin.
 */
export function useDialogueBoost(videoRef: RefObject<HTMLVideoElement>, enabled: boolean) {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const applyRef = useRef<(on: boolean) => void>(() => {});

  useEffect(() => {
    const video = videoRef.current;
    const Context = audioContextClass();
    if (!video || !Context) return;

    let graph: AudioGraph | null = null;
    let context: AudioContext | null = null;
    let wantBoost = enabledRef.current;
    let disposed = false;

    const route = () => {
      if (!graph) return;
      const { source, compressor, gain, context } = graph;
      source.disconnect();
      compressor.disconnect();
      gain.disconnect();
      if (wantBoost) {
        source.connect(compressor);
        compressor.connect(gain);
        gain.connect(context.destination);
      } else {
        source.connect(context.destination);
      }
    };

    const build = async () => {
      if (graph || !wantBoost || disposed) return;
      try {
        context = context ?? new Context();
        if (context.state !== 'running') await context.resume();
        if (disposed || graph || context.state !== 'running') return;
        const compressor = context.createDynamicsCompressor();
        compressor.threshold.value = -30;
        compressor.knee.value = 20;
        compressor.ratio.value = 4;
        compressor.attack.value = 0.005;
        compressor.release.value = 0.25;
        const gain = context.createGain();
        gain.gain.value = 1.6;
        const source = context.createMediaElementSource(video);
        graph = { context, source, compressor, gain };
        route();
      } catch {
        // Web Audio is unavailable here; the audio keeps playing directly.
      }
    };

    // A context created before any user gesture can stay suspended; try again on play.
    const resume = () => {
      if (graph) {
        if (graph.context.state !== 'running') graph.context.resume().catch(() => {});
      } else {
        void build();
      }
    };

    void build();
    video.addEventListener('play', resume);
    applyRef.current = (on) => {
      wantBoost = on;
      if (graph) route();
      else void build();
    };

    return () => {
      disposed = true;
      video.removeEventListener('play', resume);
      applyRef.current = () => {};
      context?.close().catch(() => {});
    };
  }, [videoRef]);

  useEffect(() => applyRef.current(enabled), [enabled]);
}
