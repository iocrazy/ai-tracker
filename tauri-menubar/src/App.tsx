import React, { useState, useEffect } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { invoke } from '@tauri-apps/api/core';
import { MenuBarPanel } from './MenuBarPanel';
import { FloatingBar } from './FloatingBar';
import { LoginView } from './LoginView';
import { useTrackerState } from './hooks/useTrackerState';
import { getAuthTokenAsync, setAuthToken } from './shared/services/auth';

// Separate component so useTrackerState only runs after auth token is loaded
const AuthenticatedApp: React.FC<{
  windowLabel: string;
  onLogout: () => void;
}> = ({ windowLabel, onLogout }) => {
  const trackerState = useTrackerState();

  if (windowLabel === 'float') {
    return (
      <FloatingBar
        sessions={trackerState.sessions}
        stats={trackerState.stats}
        connectionStatus={trackerState.connectionStatus}
      />
    );
  }

  return (
    <MenuBarPanel
      sessions={trackerState.sessions}
      connectionStatus={trackerState.connectionStatus}
      stats={trackerState.stats}
      onLogout={onLogout}
      onReconnect={trackerState.reconnect}
    />
  );
};

export const App: React.FC = () => {
  const [authenticated, setAuthenticated] = useState(false);
  const [loading, setLoading] = useState(true);
  const [windowLabel, setWindowLabel] = useState('panel');

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    setWindowLabel(win.label);
  }, []);

  useEffect(() => {
    (async () => {
      // Always read token from config file (server may regenerate on restart)
      try {
        const localToken = await invoke<string>('read_local_token');
        if (localToken) {
          await setAuthToken(localToken);
          setAuthenticated(true);
          setLoading(false);
          return;
        }
      } catch {
        // Config not found — try cached token
      }
      // Fallback to cached token
      const cached = await getAuthTokenAsync();
      if (cached) {
        setAuthenticated(true);
      }
      setLoading(false);
    })();
  }, []);

  if (loading) {
    return <div className="h-full" />;
  }

  if (!authenticated) {
    return <LoginView onLogin={() => setAuthenticated(true)} />;
  }

  return (
    <AuthenticatedApp
      windowLabel={windowLabel}
      onLogout={() => setAuthenticated(false)}
    />
  );
};
