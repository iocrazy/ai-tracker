import { useState, useEffect, useRef, useCallback } from 'react';
import { AgentSession, ClaudeStatus } from '../shared/types';
import { connectWebSocket, reconnectNow, disconnectWebSocket, RealtimeMessage, ConnectionStatus } from '../shared/services/state';
import { mapTmuxToSessions } from '../shared/services/dataMapper';
import { getAuthToken, API_BASE, authFetch } from '../shared/services/auth';

interface TrackerState {
  sessions: AgentSession[];
  connectionStatus: ConnectionStatus;
  retryCount: number;
  serverOnline: boolean;
}

interface TrackerStats {
  totalSessions: number;
  busyCount: number;
  idleCount: number;
  totalCost: number;
}

export function useTrackerState() {
  const [state, setState] = useState<TrackerState>({
    sessions: [],
    connectionStatus: 'offline',
    retryCount: 0,
    serverOnline: false,
  });
  const [stats, setStats] = useState<TrackerStats>({
    totalSessions: 0,
    busyCount: 0,
    idleCount: 0,
    totalCost: 0,
  });

  const wsRef = useRef<WebSocket | null>(null);
  const claudeStatusCache = useRef<Map<string, ClaudeStatus>>(new Map());

  // Fetch Claude status for ALL windows and sync IDLE↔BUSY
  const fetchAllClaudeStatus = useCallback(async (sessions: AgentSession[]) => {
    const token = getAuthToken();
    if (!token) return sessions;

    // Query Claude status for every window (not just BUSY)
    await Promise.all(
      sessions.flatMap(session =>
        session.windows.map(async (win) => {
          try {
            const params = new URLSearchParams({ session: session.name, window: win.name });
            const res = await authFetch(`${API_BASE}/tmux/claude-status?${params}`);
            if (res.ok) {
              const data = await res.json();
              if (data.success && data.status) {
                const key = `${session.id}|${win.id}`;
                // action contains spinner text when working ("✢ Twisting…"), empty/None when idle
                const action = data.status.action || '';
                const isWorking = action !== '' && action !== 'None' && action !== 'null';
                claudeStatusCache.current.set(key, data.status);
                win.claudeStatus = isWorking ? data.status : undefined;
                win.claudePane = data.status.pane || undefined;
                if (isWorking && (win.status === 'IDLE' || win.status === 'COMPLETED')) {
                  win.status = 'BUSY';
                } else if (!isWorking && win.status === 'BUSY') {
                  win.status = 'IDLE';
                }
              }
            }
          } catch {
            // Ignore individual failures
          }
        })
      )
    );

    return sessions;
  }, []);

  // Compute stats
  const computeStats = useCallback((sessions: AgentSession[]): TrackerStats => {
    let totalCost = 0;
    let busyCount = 0;
    let idleCount = 0;

    for (const s of sessions) {
      for (const w of s.windows) {
        if (w.status === 'BUSY') busyCount++;
        else if (w.status === 'IDLE') idleCount++;
        if (w.claudeStatus?.cost) totalCost += w.claudeStatus.cost;
      }
    }

    return {
      totalSessions: sessions.length,
      busyCount,
      idleCount,
      totalCost,
    };
  }, []);

  // Connect WebSocket (with retry for async token loading)
  useEffect(() => {
    const callbacks = {
      onStateUpdate: async (msg: RealtimeMessage) => {
        const sessions = mapTmuxToSessions(msg.tmux_windows, msg.state.tasks);
        const enriched = await fetchAllClaudeStatus(sessions);
        setState(prev => ({
          ...prev,
          sessions: enriched,
          serverOnline: true,
        }));
        setStats(computeStats(enriched));
      },
      onConnectionChange: (status: ConnectionStatus, retryCount?: number) => {
        setState(prev => ({
          ...prev,
          connectionStatus: status,
          retryCount: retryCount || 0,
          serverOnline: status === 'connected',
        }));
      },
    };

    const tryConnect = () => {
      const token = getAuthToken();
      if (!token) return false;
      wsRef.current = connectWebSocket(callbacks);
      return true;
    };

    // Try immediately, retry every 500ms up to 10 times if token not ready
    if (!tryConnect()) {
      let attempts = 0;
      const interval = setInterval(() => {
        attempts++;
        if (tryConnect() || attempts >= 10) {
          clearInterval(interval);
        }
      }, 500);
      return () => { clearInterval(interval); disconnectWebSocket(); wsRef.current = null; };
    }

    return () => {
      disconnectWebSocket();
      wsRef.current = null;
    };
  }, [fetchAllClaudeStatus, computeStats]);

  const reconnect = useCallback(() => {
    const ws = reconnectNow();
    if (ws) wsRef.current = ws;
  }, []);

  return { ...state, stats, reconnect };
}
