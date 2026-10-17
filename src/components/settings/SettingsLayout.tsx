import React, { useId } from 'react';

export const Section: React.FC<{
  title: string;
  children: React.ReactNode;
  /** Set on a section that can be linked to; it then takes focus when the link is followed. */
  sectionRef?: React.Ref<HTMLElement>;
}> = ({ title, children, sectionRef }) => {
  const titleId = useId();
  return (
    <section
      ref={sectionRef}
      aria-labelledby={titleId}
      tabIndex={sectionRef ? -1 : undefined}
      className="flex scroll-mt-4 flex-col gap-2 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-accent-500/60 focus-visible:ring-offset-4 focus-visible:ring-offset-bg-page"
    >
      <h2 id={titleId} className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
        {title}
      </h2>
      <div className="divide-y divide-border rounded-xl ui-card bg-bg-card px-4">{children}</div>
    </section>
  );
};

/** A labelled row; `children` receives the ids to tie the control to its label and hint. */
export const Row: React.FC<{
  label: string;
  hint?: string;
  /** Puts the control under the label, for one that needs the full width. */
  stacked?: boolean;
  children: (ids: { labelId: string; hintId: string | undefined }) => React.ReactNode;
}> = ({ label, hint, stacked = false, children }) => {
  const labelId = useId();
  const hintId = useId();
  return (
    <div
      className={
        stacked
          ? 'flex flex-col gap-3 py-3'
          : 'flex min-h-[56px] items-center justify-between gap-6 py-3'
      }
    >
      <div className="flex min-w-0 flex-col">
        <span id={labelId} className="text-[13px] font-medium text-text-primary">
          {label}
        </span>
        {hint && (
          <span id={hintId} className="mt-0.5 text-xs text-text-muted">
            {hint}
          </span>
        )}
      </div>
      <div className={stacked ? 'flex w-full' : 'flex flex-shrink-0 items-center'}>
        {children({ labelId, hintId: hint ? hintId : undefined })}
      </div>
    </div>
  );
};
