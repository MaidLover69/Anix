// WebKit's own right-click menu (Back, Forward, Stop, Reload) is no use in Anix, and Reload restarts
// the app mid-episode. It stays only where it helps: Cut/Copy/Paste in text fields and Copy on selected text.
const TEXT_FIELD = 'input, textarea, [contenteditable]:not([contenteditable="false"])';

export function keepsPageMenu(target: EventTarget | null, selectedText: string): boolean {
  if (selectedText.trim() !== '') return true;
  const el = target as { closest?: (selector: string) => unknown } | null;
  return typeof el?.closest === 'function' && el.closest(TEXT_FIELD) != null;
}

export function hidePageMenu(): void {
  document.addEventListener('contextmenu', (e) => {
    if (!keepsPageMenu(e.target, window.getSelection()?.toString() ?? '')) e.preventDefault();
  });
}
