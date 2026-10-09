import React from 'react';

interface SkeletonCardProps {
  className?: string;
}

export const SkeletonCard: React.FC<SkeletonCardProps> = ({ className = '' }) => {
  return (
    <div aria-hidden="true" className={`flex flex-col flex-shrink-0 select-none ${className}`}>
      <div className="w-full aspect-[2/3] rounded-xl shimmer-placeholder" />
      <div className="mt-2 h-3.5 w-3/4 rounded-sm shimmer-placeholder" />
      <div className="mt-1 h-3 w-1/2 rounded-sm shimmer-placeholder" />
    </div>
  );
};
