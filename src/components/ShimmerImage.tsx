import React, { useState, useEffect, useRef } from 'react';

interface ShimmerImageProps extends Omit<React.ImgHTMLAttributes<HTMLImageElement>, 'src'> {
  src?: string | null;
  alt: string;
  className?: string;
  fallbackIcon?: React.ReactNode;
}

export const ShimmerImage: React.FC<ShimmerImageProps> = ({
  src,
  alt,
  className = '',
  fallbackIcon,
  ...props
}) => {
  const [inView, setInView] = useState(false);
  // Load and error state are remembered per URL, so a new `src` starts clean.
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!('IntersectionObserver' in window)) {
      setInView(true);
      return;
    }

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setInView(true);
          observer.disconnect();
        }
      },
      { rootMargin: '200px' }
    );

    if (ref.current) {
      observer.observe(ref.current);
    }

    return () => observer.disconnect();
  }, []);

  const loaded = !!src && loadedSrc === src;
  const failed = !!src && failedSrc === src;

  return (
    <div ref={ref} className={`relative overflow-hidden ${className}`}>
      {!src || failed ? (
        <div className="absolute inset-0 flex items-center justify-center bg-bg-elevated text-text-muted">
          {fallbackIcon || <span className="text-[11px]">No image</span>}
        </div>
      ) : (
        <>
          {!loaded && <div className="absolute inset-0 shimmer-placeholder" />}
          {inView && (
            <img
              src={src}
              alt={alt}
              loading="lazy"
              draggable={false}
              onLoad={() => setLoadedSrc(src)}
              onError={() => setFailedSrc(src)}
              className={`w-full h-full object-cover transition-opacity duration-200 ${
                loaded ? 'opacity-100' : 'opacity-0'
              }`}
              {...props}
            />
          )}
        </>
      )}
    </div>
  );
};
