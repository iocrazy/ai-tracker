import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './index.css'

// Auto-reload when a new Service Worker takes control (ensures fresh JS after deploy)
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    window.location.reload();
  });
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
