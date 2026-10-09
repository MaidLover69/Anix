import React from 'react';
import { Modal } from './ui/Modal';
import type { Trailer } from '../types';

interface TrailerModalProps {
  isOpen: boolean;
  onClose: () => void;
  trailer?: Trailer | null;
  animeTitle: string;
}

/** True when the trailer can be embedded (only YouTube is supported). */
export function isPlayableTrailer(trailer?: Trailer | null): boolean {
  return !!trailer?.id && trailer.site?.toLowerCase() === 'youtube';
}

export const TrailerModal: React.FC<TrailerModalProps> = ({
  isOpen,
  onClose,
  trailer,
  animeTitle,
}) => {
  if (!trailer || !isPlayableTrailer(trailer)) return null;

  const embedUrl = `https://www.youtube-nocookie.com/embed/${trailer.id}?autoplay=1&rel=0`;

  return (
    <Modal isOpen={isOpen} onClose={onClose} title={`${animeTitle} trailer`} size="2xl">
      <div className="aspect-video w-full overflow-hidden rounded-xl bg-black">
        <iframe
          src={embedUrl}
          title={`${animeTitle} trailer`}
          allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
          allowFullScreen
          className="h-full w-full border-0"
        />
      </div>
    </Modal>
  );
};
