import './theme/bootstrap';
import React from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { App } from './App';
import { initI18n } from './i18n';
import { hidePageMenu } from './lib/pageMenu';
import '@fontsource-variable/inter';
// The real italic face: without it WebKit slants the upright one, which smears Meme mode's captions.
import '@fontsource-variable/inter/wght-italic.css';
import './theme/global.css';
import './theme/themes.css';
import './theme/browse.css';
import './theme/detail.css';
import './theme/lists.css';
import './theme/fun.css';

hidePageMenu();

// WebKitGTK aborts the page when a view transition starts with compositing off (safe mode, or the
// user's WEBKIT_DISABLE_COMPOSITING_MODE). Without the method, react-router just navigates.
const viewTransitionsChecked = invoke<boolean>('webkit_compositing_off').then(
  (off) => {
    if (off) delete (Document.prototype as { startViewTransition?: unknown }).startViewTransition;
  },
  () => undefined
);

// The language remembered from the last run is loaded first, so the first frame is already in it.
void Promise.all([initI18n(), viewTransitionsChecked]).finally(() =>
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
);
