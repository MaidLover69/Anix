import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Layers, Play, Search, Square, SquareCheck, Trash2, X } from 'lucide-react';
import { Button } from './ui/Button';
import { sameEpisode } from './player/format';
import type { EpisodeItem } from '../types';

interface PlayerPlaylistDrawerProps {
  isOpen: boolean;
  onClose: () => void;
  episodes: EpisodeItem[];
  currentEpisodeNumber: number;
  onSelectEpisode: (epNum: number) => void;
  queuedEpisodes: number[];
  onToggleQueueEpisode: (epNum: number) => void;
  onPlayQueue: () => void;
  onClearQueue: () => void;
  /** Portal target; the player passes the element it shows fullscreen. */
  container?: HTMLElement | null;
}

const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])';

type DrawerPanelProps = Omit<PlayerPlaylistDrawerProps, 'isOpen' | 'container'>;

// Mounted only while the drawer is open, so the search text and the multi-select mode
// start fresh every time.
const DrawerPanel: React.FC<DrawerPanelProps> = ({
  onClose,
  episodes,
  currentEpisodeNumber,
  onSelectEpisode,
  queuedEpisodes,
  onToggleQueueEpisode,
  onPlayQueue,
  onClearQueue,
}) => {
  const [searchQuery, setSearchQuery] = useState('');
  const [multiSelect, setMultiSelect] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const currentRowRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  // Focus moves into the drawer, stays inside it, and returns to where it was on close.
  useEffect(() => {
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = panelRef.current;
    (currentRowRef.current ?? panel)?.focus({ preventScroll: true });
    currentRowRef.current?.scrollIntoView({ block: 'center' });

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onCloseRef.current();
        return;
      }
      if (e.key !== 'Tab' || !panel) return;
      const focusable = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (!panel.contains(active) || active === panel) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
      } else if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      if (trigger && document.contains(trigger)) trigger.focus();
    };
  }, []);

  const filteredEpisodes = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    if (!query) return episodes;
    return episodes.filter(
      (ep) => ep.number.toString().includes(query) || ep.title?.toLowerCase().includes(query)
    );
  }, [episodes, searchQuery]);

  return (
    <div
      ref={panelRef}
      role="dialog"
      aria-modal="true"
      aria-label="Episodes"
      tabIndex={-1}
      onClick={(e) => e.stopPropagation()}
      className="flex h-full w-full max-w-sm flex-col bg-bg-elevated shadow-pop outline-none animate-slide-left"
    >
      <div className="flex items-center justify-between gap-3 px-4 pb-3 pt-4">
        <div className="min-w-0">
          <h2 className="text-base font-semibold text-text-primary">Episodes</h2>
          <p className="text-xs text-text-muted">
            {episodes.length} {episodes.length === 1 ? 'episode' : 'episodes'}
          </p>
        </div>
        <div className="flex items-center gap-1">
          <Button
            icon
            size="sm"
            variant={multiSelect ? 'secondary' : 'ghost'}
            aria-label="Select episodes to queue"
            aria-pressed={multiSelect}
            title="Select episodes to queue"
            onClick={() => setMultiSelect((prev) => !prev)}
          >
            <Layers aria-hidden="true" className="h-4 w-4" />
          </Button>
          <Button icon size="sm" variant="ghost" aria-label="Close" title="Close" onClick={onClose}>
            <X aria-hidden="true" className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {multiSelect && (
        <div className="flex items-center justify-between gap-2 bg-accent-tint px-4 py-2">
          <span className="text-xs font-medium text-accent-300">{queuedEpisodes.length} queued</span>
          <div className="flex items-center gap-1">
            <Button
              icon
              size="sm"
              variant="ghost"
              aria-label="Clear the queue"
              title="Clear the queue"
              disabled={queuedEpisodes.length === 0}
              onClick={onClearQueue}
            >
              <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={queuedEpisodes.length === 0}
              onClick={() => {
                onPlayQueue();
                onClose();
              }}
            >
              <Play aria-hidden="true" className="h-3 w-3 fill-current" />
              Play queue
            </Button>
          </div>
        </div>
      )}

      <div className="px-4 pb-2 pt-1">
        <div className="relative">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-text-muted"
          />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search by number or title"
            aria-label="Search episodes"
            className="h-9 w-full rounded-md border border-border bg-bg-input pl-9 pr-3 text-[13px] text-text-primary placeholder:text-text-muted"
          />
        </div>
      </div>

      <div className="flex flex-1 flex-col gap-1 overflow-y-auto px-4 pb-4 pt-1">
        {filteredEpisodes.length === 0 ? (
          <p className="py-10 text-center text-[13px] text-text-muted">
            {episodes.length === 0 ? 'The episode list is not available.' : 'No episode matches the search.'}
          </p>
        ) : (
          filteredEpisodes.map((ep) => {
            const isCurrent = sameEpisode(ep.number, currentEpisodeNumber);
            const isQueued = queuedEpisodes.some((n) => sameEpisode(n, ep.number));
            const title = ep.title && !/^episode\s+[\d.]+$/i.test(ep.title.trim()) ? ep.title : null;

            return (
              <button
                key={ep.number}
                ref={isCurrent ? currentRowRef : undefined}
                type="button"
                aria-current={isCurrent ? 'true' : undefined}
                aria-pressed={multiSelect ? isQueued : undefined}
                onClick={() => {
                  if (multiSelect) {
                    onToggleQueueEpisode(ep.number);
                  } else {
                    onSelectEpisode(ep.number);
                    onClose();
                  }
                }}
                className={`flex min-h-[2.75rem] flex-shrink-0 items-center gap-3 rounded-md px-3 py-1.5 text-left cursor-pointer transition-colors ${
                  isCurrent
                    ? 'bg-accent-tint text-accent-300'
                    : 'ui-inset bg-bg-surface text-text-secondary hover:bg-bg-input hover:text-text-primary'
                }`}
              >
                {multiSelect &&
                  (isQueued ? (
                    <SquareCheck aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-accent-300" />
                  ) : (
                    <Square aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
                  ))}
                <span className="w-8 flex-shrink-0 text-[13px] font-semibold tabular-nums">{ep.number}</span>
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-[13px]">{title ?? `Episode ${ep.number}`}</span>
                  {ep.filler && <span className="text-[11px] text-state-warning">Filler</span>}
                </span>
                {isCurrent ? (
                  <span className="flex-shrink-0 text-[11px] font-semibold">Playing</span>
                ) : (
                  isQueued && !multiSelect && (
                    <span className="flex-shrink-0 text-[11px] text-text-muted">Queued</span>
                  )
                )}
              </button>
            );
          })
        )}
      </div>
    </div>
  );
};

export const PlayerPlaylistDrawer: React.FC<PlayerPlaylistDrawerProps> = ({
  isOpen,
  container,
  ...panelProps
}) => {
  if (!isOpen) return null;

  return createPortal(
    <div
      className="absolute inset-0 z-[60] flex justify-end bg-black/60 animate-fade-in"
      onClick={panelProps.onClose}
    >
      <DrawerPanel {...panelProps} />
    </div>,
    container ?? document.body
  );
};
