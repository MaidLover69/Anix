import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { ArrowLeft, ArrowRight } from 'lucide-react';
import clsx from 'clsx';
import { Modal } from './ui/Modal';
import { Button } from './ui/Button';
import { Spinner } from './ui/Spinner';
import { ShimmerImage } from './ShimmerImage';
import { errorMessage } from './ui/ErrorState';
import { mediaFormatLabel } from './library/format';
import { useMediaQuery } from '../hooks/useMediaQuery';
import { t } from '../i18n';
import type { Media } from '../types';

interface SurpriseCardProps {
  isOpen: boolean;
  onClose: () => void;
  /** A fresh batch of picks to draw from (a random page of well-rated titles). */
  draw: () => Promise<Media[]>; // i18n-ignore
}

/** How far the card is dragged before letting go skips (left) or opens (right) it. */
const SWIPE_PX = 110;
const FLING_MS = 180;

function plainText(html: string | null): string {
  return (html ?? '').replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim();
}

const CardBody: React.FC<Omit<SurpriseCardProps, 'isOpen'>> = ({ onClose, draw }) => {
  const navigate = useNavigate();
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  const [pick, setPick] = useState<Media | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [dx, setDx] = useState(0);
  const [flinging, setFlinging] = useState<'left' | 'right' | null>(null);
  const queue = useRef<Media[]>([]);
  const seen = useRef(new Set<number>());
  const drag = useRef<{ x: number; id: number } | null>(null);

  const next = useCallback(async () => {
    setError(null);
    try {
      // A batch can be all seen already; a few more draws find new ones.
      for (let tries = 0; queue.current.length === 0 && tries < 3; tries++) {
        const fresh = (await draw()).filter((m) => !seen.current.has(m.id));
        queue.current = fresh.sort(() => Math.random() - 0.5);
      }
      const nextPick = queue.current.shift();
      if (!nextPick) throw new Error(t('surpriseCard.seenEverything'));
      seen.current.add(nextPick.id);
      setPick(nextPick);
    } catch (err) {
      setError(err);
    }
    // Reset together with the new pick, so the flung card does not snap back while the batch loads.
    setDx(0);
    setFlinging(null);
  }, [draw]);

  useEffect(() => {
    void next();
  }, [next]);

  const open = useCallback(() => {
    if (!pick) return;
    navigate(`/detail/${pick.id}`);
    onClose();
  }, [pick, navigate, onClose]);

  const leave = (direction: 'left' | 'right') => {
    if (!pick || flinging) return;
    const done = direction === 'left' ? () => void next() : open;
    if (reducedMotion) return done();
    setFlinging(direction);
    window.setTimeout(done, FLING_MS);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowLeft') {
      e.preventDefault();
      leave('left');
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      leave('right');
    }
  };

  const offset = flinging === 'left' ? -480 : flinging === 'right' ? 480 : dx;
  const caption = pick
    ? [mediaFormatLabel(pick.format), pick.season_year, pick.average_score ? `${pick.average_score}%` : null]
        .filter(Boolean)
        .join(' · ')
    : '';

  return (
    <div onKeyDown={onKeyDown} className="flex flex-col gap-4">
      <div className="relative flex min-h-[300px] items-center justify-center overflow-hidden">
        {error ? (
          <p role="alert" className="text-center text-[13px] text-state-error">
            {t('surpriseCard.couldNotPick', { error: errorMessage(error) })}
          </p>
        ) : !pick ? (
          <Spinner label={t('surpriseCard.picking')} />
        ) : (
          <div
            key={pick.id}
            aria-live="polite"
            onPointerDown={(e) => {
              if (e.button !== 0 || flinging) return;
              drag.current = { x: e.clientX, id: e.pointerId };
              e.currentTarget.setPointerCapture(e.pointerId);
            }}
            onPointerMove={(e) => {
              if (drag.current?.id === e.pointerId) setDx(e.clientX - drag.current.x);
            }}
            onPointerUp={(e) => {
              if (drag.current?.id !== e.pointerId) return;
              drag.current = null;
              if (dx <= -SWIPE_PX) leave('left');
              else if (dx >= SWIPE_PX) leave('right');
              else setDx(0);
            }}
            onPointerCancel={() => {
              drag.current = null;
              setDx(0);
            }}
            style={{
              transform: `translateX(${offset}px) rotate(${offset / 30}deg)`,
              opacity: flinging ? 0 : 1,
            }}
            className={clsx(
              'ui-card flex w-full max-w-sm cursor-grab touch-none select-none gap-4 rounded-xl border border-border bg-bg-card p-3 active:cursor-grabbing',
              // Following the pointer has no delay; letting go eases back or away.
              drag.current === null && !reducedMotion && 'transition-[transform,opacity] duration-200 ease-out'
            )}
          >
            <ShimmerImage
              src={pick.cover_image.large || pick.cover_image.extra_large}
              alt=""
              className="pointer-events-none aspect-[2/3] w-28 flex-shrink-0 rounded-md"
            />
            <div className="flex min-w-0 flex-col gap-1">
              <h3 className="text-[15px] font-semibold leading-5 text-text-primary line-clamp-3">
                {pick.title.english || pick.title.user_preferred || pick.title.romaji || t('surpriseCard.untitled')}
              </h3>
              {caption && <p className="text-xs text-text-muted">{caption}</p>}
              {pick.genres.length > 0 && <p className="text-xs text-text-secondary">{pick.genres.slice(0, 3).join(', ')}</p>}
              <p className="mt-1 text-xs leading-[18px] text-text-muted line-clamp-5">{plainText(pick.description)}</p>
            </div>
          </div>
        )}
      </div>
      {/* Left to right in every language: skip flings left (←), open flings right (→). */}
      <div dir="ltr" className="flex items-center justify-between gap-3">
        <Button variant="neutral" onClick={() => (error ? void next() : leave('left'))} disabled={!pick && !error}>
          <ArrowLeft aria-hidden="true" className="h-4 w-4" />
          {error ? t('common.tryAgain') : t('surpriseCard.skip')}
        </Button>
        <span dir="auto" className="text-xs text-text-muted">
          {t('surpriseCard.dragHint')}
        </span>
        <Button variant="primary" onClick={() => leave('right')} disabled={!pick}>
          {t('surpriseCard.open')}
          <ArrowRight aria-hidden="true" className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
};

/** "Surprise me": one random pick at a time; skip it (←, drag left) or open it (→, drag right). */
export const SurpriseCard: React.FC<SurpriseCardProps> = ({ isOpen, onClose, draw }) => (
  <Modal isOpen={isOpen} onClose={onClose} title={t('surpriseCard.title')} size="md">
    {/* Mounted only while open, so each visit starts with a new pick. */}
    {isOpen && <CardBody onClose={onClose} draw={draw} />}
  </Modal>
);
