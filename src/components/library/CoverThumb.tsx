import React from 'react';
import { ImageOff } from 'lucide-react';
import { ShimmerImage } from '../ShimmerImage';

interface CoverThumbProps {
  src: string | null | undefined;
  /** Size and radius of the thumbnail. */
  className: string;
}

/** A decorative cover; a missing or broken image becomes an icon that fits small sizes. */
export const CoverThumb: React.FC<CoverThumbProps> = ({ src, className }) => (
  <ShimmerImage
    src={src}
    alt=""
    className={className}
    fallbackIcon={<ImageOff className="w-4 h-4" aria-hidden="true" />}
  />
);
