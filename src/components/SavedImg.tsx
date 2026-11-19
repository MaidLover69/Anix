import React, { useEffect, useState } from 'react';
import { rememberImage, savedImage } from '../lib/savedImages';

type Props = Omit<React.ImgHTMLAttributes<HTMLImageElement>, 'src' | 'onError'> & { src: string; onFailed?: () => void };

/** An `<img>` that saves what it loads on disk and, when the address cannot be loaded, shows the saved copy. */
export const SavedImg: React.FC<Props> = ({ src, onFailed, onLoad, ...props }) => {
  const [data, setData] = useState<string | null>(null);
  useEffect(() => setData(null), [src]);
  return (
    <img
      {...props}
      src={data ?? src}
      onLoad={(event) => {
        if (!data) rememberImage(src);
        onLoad?.(event);
      }}
      onError={() => {
        if (data) return onFailed?.();
        savedImage(src).then((saved) => (saved ? setData(saved) : onFailed?.()));
      }}
    />
  );
};
