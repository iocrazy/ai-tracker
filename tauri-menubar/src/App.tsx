import React, { useState, useEffect } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { MenuBarPanel } from './MenuBarPanel';
import { FloatingBar } from './FloatingBar';
import { LoginView } from './LoginView';
import { useTrackerState } from './hooks/useTrackerState';
import { getAuthTokenAsync } from './shared/services/auth';

export const App: React.FC = () => {
  const [authenticated, setAuthenticated] = useState(false);
  const [loading, setLoading] = useState(true);
  const [windowLabel, setWindowLabel] = useState('panel');

  // Determine which window we're in
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    setWindowLabel(win.label);
  }, []);

  // Check for existing token
  useEffect(() => {
    getAuthTokenAsync().then(token => {
      setAuthenticated(!!token);
      setLoading(false);
    });
  }, []);

  const trackerState = useTrackerState();

  if (loading) {
    return <div className="h-full bg-neutral-900" />;
  }

  if (!authenticated) {
    return <LoginView onLogin={() => setAuthenticated(true)} />;
  }

  if (windowLabel === 'float') {
    return (
      <FloatingBar
        sessions={trackerState.sessions}
        stats={trackerState.stats}
        connectionStatus={trackerState.connectionStatus}
      />
    );
  }

  // Default: panel view
  return (
    <MenuBarPanel
      sessions={trackerState.sessions}
      connectionStatus={trackerState.connectionStatus}
      stats={trackerState.stats}
    />
  );
};
