import { useState, useEffect, useRef, useCallback } from 'react';
import { AgentSession, ClaudeStatus } from '../shared/types';
import { connectWebSocket, RealtimeMessage, ConnectionStatus } from '../shared/services/state';
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

  // Fetch Claude status for all BUSY windows
  const fetchAllClaudeStatus = useCallback(async (sessions: AgentSession[]) => {
    const token = getAuthToken();
    if (!token) return sessions;

    const busyWindows: { session: AgentSession; window: AgentSession['windows'][0] }[] = [];
    for (const s of sessions) {
      for (const w of s.windows) {
        if (w.status === 'BUSY' || w.status === 'PAUSED') {
          busyWindows.push({ session: s, window: w });
        }
      }
    }

    await Promise.all(
      busyWindows.map(async ({ session, window: win }) => {
        try {
          const params = new URLSearchParams({ session: session.name, window: win.name });
          const res = await authFetch(`${API_BASE}/tmux/claude-status?${params}`);
          if (res.ok) {
            const data = await res.json();
            if (data.success && data.status) {
              const key = `${session.id}|${win.id}`;
              claudeStatusCache.current.set(key, data.status);
              win.claudeStatus = data.status;
              win.claudePane = data.status.pane || undefined;
            }
          }
        } catch {
          // Ignore individual failures
        }
      })
    );

    // Apply cached status to non-busy windows too
    for (const s of sessions) {
      for (const w of s.windows) {
        if (!w.claudeStatus) {
          const key = `${s.id}|${w.id}`;
          const cached = claudeStatusCache.current.get(key);
          if (cached) w.claudeStatus = cached;
        }
      }
    }

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

  // Connect WebSocket
  useEffect(() => {
    const token = getAuthToken();
    if (!token) return;

    wsRef.current = connectWebSocket({
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
      onConnectionChange: (status, retryCount) => {
        setState(prev => ({
          ...prev,
          connectionStatus: status,
          retryCount: retryCount || 0,
          serverOnline: status === 'connected',
        }));
      },
    });

    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [fetchAllClaudeStatus, computeStats]);

  return { ...state, stats };
}
