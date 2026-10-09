import React from 'react';
import { Download } from 'lucide-react';
import { Dialog } from './Dialog';
import { Button } from './ui/Button';
import { useUIStore } from '../stores/uiStore';
import { useCheckUpdate } from '../hooks/useSettings';
import { useOpenExternal } from '../hooks/useOpenExternal';

export const UpdateDialog: React.FC = () => {
  const updateDialogOpen = useUIStore((s) => s.updateDialogOpen);
  const setUpdateDialogOpen = useUIStore((s) => s.setUpdateDialogOpen);
  const { data: updateInfo } = useCheckUpdate();
  const openExternal = useOpenExternal();

  if (!updateInfo) return null;

  const published = updateInfo.published_at ? new Date(updateInfo.published_at) : null;
  const publishedLabel =
    published && !Number.isNaN(published.getTime()) ? published.toLocaleDateString() : null;

  return (
    <Dialog
      isOpen={updateDialogOpen}
      onClose={() => setUpdateDialogOpen(false)}
      title={`Anilili ${updateInfo.version} is available`}
      maxWidth="max-w-md"
      footer={
        <>
          <Button variant="ghost" onClick={() => setUpdateDialogOpen(false)}>
            Later
          </Button>
          <Button
            variant="primary"
            data-autofocus
            onClick={() => {
              openExternal(updateInfo.release_url);
              setUpdateDialogOpen(false);
            }}
          >
            <Download aria-hidden="true" className="h-4 w-4" />
            Open download page
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-2">
        {publishedLabel && <p className="text-xs text-text-muted">Released {publishedLabel}</p>}
        <h3 className="text-[13px] font-medium text-text-primary">Release notes</h3>
        <div className="max-h-56 overflow-y-auto whitespace-pre-wrap rounded-md bg-bg-card p-3 text-[13px] leading-relaxed text-text-secondary select-text">
          {updateInfo.changelog || 'No release notes were published.'}
        </div>
      </div>
    </Dialog>
  );
};
