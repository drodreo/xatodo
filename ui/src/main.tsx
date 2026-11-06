import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';

// Standalone dev/debug entry (vite dev); not part of the build artifact
// (the build input is entry.tsx).
// Opened directly in a browser there is no shell env (__SL_SHELL__ missing),
// and App renders a missing-env notice instead of failing silently.
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
