import React from 'react'
import ReactDOM from 'react-dom/client'
import { registerSW } from 'virtual:pwa-register'
import App from './App'
import './index.css'

// Register Service Worker with update prompt
const updateSW = registerSW({
  onNeedRefresh() {
    // Dispatch custom event so App.tsx can show an update banner
    window.dispatchEvent(new CustomEvent('sw-update-available', { detail: { updateSW } }));
  },
  onOfflineReady() {
    console.log('PWA: offline ready');
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
