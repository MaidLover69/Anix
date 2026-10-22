import './theme/bootstrap';
import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { initI18n } from './i18n';
import '@fontsource-variable/inter';
import './theme/global.css';
import './theme/themes.css';

// The language remembered from the last run is loaded first, so the first frame is already in it.
void initI18n().finally(() =>
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
);
