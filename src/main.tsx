import './theme/bootstrap';
import React from 'react';
import ReactDOM from 'react-dom/client';
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

// The language remembered from the last run is loaded first, so the first frame is already in it.
void initI18n().finally(() =>
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
);
