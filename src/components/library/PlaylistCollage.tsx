import React, { useState } from 'react';
import { ListVideo } from 'lucide-react';

interface PlaylistCollageProps {
  covers: string[] | null | undefined;
  /** Sets the size and radius; the collage fills it. */
  className?: string;
}

/**
 * Up to four covers laid out so no cell is left empty: one cover fills the tile, two sit side
 * by side, three put the first in a full-height column. Covers that fail to load are dropped.
 */
export const PlaylistCollage: React.FC<PlaylistCollageProps> = ({ covers, className = '' }) => {
  const [failed, setFailed] = useState<string[]>([]);

  const usable = Array.from(new Set(covers ?? []))
    .filter((cover) => cover && !failed.includes(cover))
    .slice(0, 4);

  if (usable.length === 0) {
    return (
      <div className={`flex items-center justify-center bg-bg-elevated text-text-disabled ${className}`}>
        <ListVideo className="w-7 h-7" aria-hidden="true" />
      </div>
    );
  }

  const columns = usable.length === 1 ? 'grid-cols-1' : 'grid-cols-2';
  const rows = usable.length <= 2 ? 'grid-rows-1' : 'grid-rows-2';

  return (
    <div className={`grid gap-px overflow-hidden bg-bg-elevated ${columns} ${rows} ${className}`}>
      {usable.map((cover, index) => (
        <img
          key={cover}
          src={cover}
          alt=""
          loading="lazy"
          draggable={false}
          onError={() => setFailed((current) => [...current, cover])}
          className={`w-full h-full min-h-0 object-cover ${
            usable.length === 3 && index === 0 ? 'row-span-2' : ''
          }`}
        />
      ))}
    </div>
  );
};
