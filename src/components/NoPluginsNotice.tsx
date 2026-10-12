import React from 'react';
import { useNavigate } from 'react-router-dom';
import { Puzzle } from 'lucide-react';
import clsx from 'clsx';
import { Button } from './ui/Button';
import { PLUGIN_SETTINGS_PATH } from '../hooks/usePlugins';

/** What is missing, in one line: nothing installed, or nothing switched on. */
export function noPluginsTitle(installed: number): string {
  return installed === 0 ? 'No streaming plugin is installed' : 'No streaming plugin is enabled';
}

export function noPluginsDetail(installed: number): string {
  return installed === 0
    ? 'Anix plays episodes through plugins. Add a plugin repository in Settings to start watching.'
    : 'Anix plays episodes through plugins. Turn one on in Settings, or check the ones that failed to load.';
}

interface NoPluginsNoticeProps {
  /** Plugins installed in any state; decides the wording. */
  installed: number;
  /** Replaces the default second sentence, to say what cannot be done here. */
  detail?: string;
  /** Runs before the app moves to Settings, for a dialog to close itself. */
  onOpenSettings?: () => void;
  className?: string;
}

/** Inline note for places that need a streaming source when no plugin can provide one. */
export const NoPluginsNotice: React.FC<NoPluginsNoticeProps> = ({
  installed,
  detail,
  onOpenSettings,
  className,
}) => {
  const navigate = useNavigate();
  return (
    <div
      role="status"
      className={clsx(
        'flex flex-wrap items-center gap-x-4 gap-y-2 rounded-md border border-border ui-inset bg-bg-surface px-3 py-2.5',
        className
      )}
    >
      <Puzzle aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
      <div className="min-w-0 flex-1 basis-56">
        <p className="text-[13px] font-medium text-text-primary">{noPluginsTitle(installed)}</p>
        <p className="mt-0.5 text-xs text-text-muted">{detail ?? noPluginsDetail(installed)}</p>
      </div>
      <Button
        variant="secondary"
        size="sm"
        onClick={() => {
          onOpenSettings?.();
          navigate(PLUGIN_SETTINGS_PATH);
        }}
      >
        Open plugin settings
      </Button>
    </div>
  );
};
