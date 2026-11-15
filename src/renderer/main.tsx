import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import '@fontsource/ibm-plex-sans/400.css';
import '@fontsource/ibm-plex-sans/500.css';
import '@fontsource/ibm-plex-sans/600.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import './theme.css';
import './index.css';
import { applyTheme } from './theme';

// System theme right away (no flash), then the saved preference.
applyTheme('system');
void window.electron?.getSettings?.().then((s: { theme?: 'system' | 'light' | 'dark' } | null) => applyTheme(s?.theme));

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
