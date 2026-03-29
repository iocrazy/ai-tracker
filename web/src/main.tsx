import React from 'react'
import ReactDOM from 'react-dom/client'
import { registerSW } from 'virtual:pwa-register'
import App from './App'
import './index.css'

// Auto-update: new SW activates immediately and page reloads
registerSW({
  immediate: true,
  onRegisteredSW(_swUrl, registration) {
    // Check for updates every 60 seconds
    if (registration) {
      setInterval(() => registration.update(), 60 * 1000);
    }
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
