import React, { useState, useEffect, useCallback } from 'react';
import { Monitor, Pin, Globe, LogOut, Power, ChevronRight, Eye, Server, Key, Check, X, RefreshCw, Copy } from 'lucide-react';
import { AgentSession, AgentWindow, ClaudeStatus } from './shared/types';
import { invoke } from '@tauri-apps/api/core';
import { clearAuthToken, setAuthToken, API_BASE, authFetch } from './shared/services/auth';

interface MenuBarPanelProps {
  sessions: AgentSession[];
  connectionStatus: 'connected' | 'reconnecting' | 'offline';
  stats: {
    totalSessions: number;
    busyCount: number;
    totalCost: number;
  };
  onLogout?: () => void;
  onReconnect?: () => void;
}

const STATUS_COLOR: Record<AgentWindow['status'], string> = {
  BUSY: 'text-yellow-500',
  PAUSED: 'text-orange-500',
  IDLE: 'text-green-500',
  COMPLETED: 'text-cyan-500',
  OFFLINE: 'text-gray-400',
};

const STATUS_LABEL: Record<AgentWindow['status'], string> = {
  BUSY: 'busy',
  PAUSED: 'waiting',
  IDLE: 'idle',
  COMPLETED: 'done',
  OFFLINE: 'offline',
};

const ClaudeDetail: React.FC<{ status: ClaudeStatus }> = ({ status }) => (
  <div className="menu-row pl-[52px] pr-3 py-0.5 text-[11px] text-gray-500 pointer-events-none overflow-hidden">
    {(status.current_tool || status.action) && (
      <div className="text-orange-600 truncate">{status.current_tool || status.action}</div>
    )}
    <div className="flex items-center gap-1 truncate">
      {status.model && <span>{status.model.replace('claude-', '').split('-')[0]}</span>}
      {status.cost != null && <span>${status.cost.toFixed(2)}</span>}
      {status.context_percent != null && (
        <span className={status.context_percent > 80 ? 'text-orange-500' : ''}>
          {status.context_percent.toFixed(0)}%
        </span>
      )}
    </div>
  </div>
);

const selectTmuxWindow = (sessionName: string, windowName: string, windowId?: string) => {
  authFetch(`${API_BASE}/tmux/select-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session: sessionName, window: windowName, window_id: windowId }),
  }).catch(() => {});
};

const WindowItem: React.FC<{ win: AgentWindow; sessionName: string }> = ({ win, sessionName }) => {
  const color = STATUS_COLOR[win.status];
  return (
    <>
      <button
        onClick={() => selectTmuxWindow(sessionName, win.name, win.id)}
        className="menu-item w-full flex items-center pl-[40px] pr-3 py-[3px]"
      >
        <span className={`text-[7px] ${color} mr-2`}>{'\u25CF'}</span>
        <span className="text-[13px] text-gray-800 truncate flex-1 text-left">{win.name}</span>
        <span className={`text-[11px] ${color} ml-2`}>{STATUS_LABEL[win.status]}</span>
      </button>
      {win.claudeStatus && (win.status === 'BUSY' || win.status === 'PAUSED') && (
        <ClaudeDetail status={win.claudeStatus} />
      )}
    </>
  );
};

const SessionItem: React.FC<{ session: AgentSession }> = ({ session }) => {
  const [expanded, setExpanded] = useState(session.status === 'BUSY');
  const busyCount = session.windows.filter(w => w.status === 'BUSY').length;

  return (
    <>
      <button
        onClick={() => setExpanded(!expanded)}
        className="menu-item w-full flex items-center gap-2 px-3 py-[5px]"
      >
        <Monitor className="w-4 h-4 text-gray-500 shrink-0" />
        <span className="text-[13px] text-gray-800 truncate flex-1 text-left">{session.name}</span>
        {busyCount > 0 && (
          <span className="text-[11px] text-yellow-600 font-medium shrink-0">{busyCount} busy</span>
        )}
        <ChevronRight className={`w-3 h-3 text-gray-400 shrink-0 transition-transform ${expanded ? 'rotate-90' : ''}`} />
      </button>
      {expanded && session.windows.map(win => (
        <WindowItem key={win.id} win={win} sessionName={session.name} />
      ))}
    </>
  );
};

const Separator: React.FC = () => (
  <div className="my-1 mx-2 border-t border-black/8" />
);

const MenuItem: React.FC<{
  icon: React.FC<{ className?: string }>;
  label: string;
  onClick: () => void;
  shortcut?: string;
  danger?: boolean;
}> = ({ icon: Icon, label, onClick, shortcut, danger }) => (
  <button
    onClick={onClick}
    className="menu-item w-full flex items-center gap-2 px-3 py-[5px]"
  >
    <Icon className={`w-4 h-4 ${danger ? 'text-gray-400' : 'text-gray-500'} shrink-0`} />
    <span className={`text-[13px] ${danger ? 'text-gray-500' : 'text-gray-800'} flex-1 text-left`}>{label}</span>
    {shortcut && <span className="text-[11px] text-gray-400">{shortcut}</span>}
  </button>
);

const OPACITY_KEY = 'float_opacity';

interface ServerStatus {
  source: 'sidecar' | 'external' | 'offline' | 'unknown';
  port: number;
  running: boolean;
}

interface HealthInfo {
  uptime: string;
  dbSize: string;
  tmuxSessions: number;
}

export const MenuBarPanel: React.FC<MenuBarPanelProps> = ({ sessions, connectionStatus, stats, onLogout, onReconnect }) => {
  const isOnline = connectionStatus === 'connected';
  const [floatOpacity, setFloatOpacity] = useState(() => {
    const saved = localStorage.getItem(OPACITY_KEY);
    return saved ? parseFloat(saved) : 1.0;
  });
  const [serverStatus, setServerStatus] = useState<ServerStatus | null>(null);
  const [health, setHealth] = useState<HealthInfo | null>(null);
  const [editingToken, setEditingToken] = useState(false);
  const [tokenValue, setTokenValue] = useState('');
  const [tokenSaved, setTokenSaved] = useState(false);

  // Fetch server status and health on mount + periodically
  const fetchStatus = useCallback(async () => {
    try {
      const status = await invoke<ServerStatus>('get_server_status');
      setServerStatus(status);
    } catch { /* ignore */ }
    try {
      const resp = await fetch(`${API_BASE}/health`);
      if (resp.ok) {
        const data = await resp.json();
        setHealth({
          uptime: data.checks?.uptime || data.uptime || '',
          dbSize: data.checks?.database?.size || '',
          tmuxSessions: data.checks?.tmux?.sessions ?? 0,
        });
      }
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    fetchStatus();
    const interval = setInterval(fetchStatus, 30000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  useEffect(() => {
    localStorage.setItem(OPACITY_KEY, String(floatOpacity));
    invoke('set_float_opacity', { opacity: floatOpacity }).catch(() => {});
  }, [floatOpacity]);

  const handlePinFloat = async () => {
    try { await invoke('show_float'); } catch (e) { console.error('show_float failed:', e); }
    setTimeout(() => {
      invoke('set_float_opacity', { opacity: floatOpacity }).catch(() => {});
    }, 100);
  };

  const handleOpenDashboard = () => {
    invoke('open_dashboard').catch(console.error);
  };

  const handleLogout = async () => {
    await clearAuthToken();
    onLogout?.();
  };

  const handleEditToken = async () => {
    if (!editingToken) {
      // Load current token from config file
      try {
        const token = await invoke<string>('read_local_token');
        setTokenValue(token);
      } catch {
        setTokenValue('');
      }
      setEditingToken(true);
      setTokenSaved(false);
    }
  };

  const handleSaveToken = async () => {
    if (!tokenValue.trim()) return;
    try {
      // Save to server config (agent-config.json)
      await invoke('save_local_token', { token: tokenValue.trim() });
      // Also update the Tauri app's auth token
      await setAuthToken(tokenValue.trim());
      setTokenSaved(true);
      setTimeout(() => {
        setEditingToken(false);
        setTokenSaved(false);
      }, 1000);
    } catch (e) {
      console.error('save token failed:', e);
    }
  };

  const handleRandomizeToken = () => {
    const bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    const hex = Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
    setTokenValue(hex);
  };

  const handleCancelToken = () => {
    setEditingToken(false);
    setTokenSaved(false);
  };

  const sourceLabel = serverStatus?.source === 'sidecar' ? 'Sidecar'
    : serverStatus?.source === 'external' ? 'External'
    : serverStatus?.source === 'offline' ? 'Offline'
    : '...';

  return (
    <div className="flex flex-col h-full select-none rounded-[10px] overflow-hidden py-1">
      {/* Stats header */}
      <div className="flex items-center gap-2 px-3 py-[5px]">
        <span className={`text-[8px] ${isOnline ? 'text-green-500' : 'text-red-500'}`}>{'\u25CF'}</span>
        {!isOnline ? (
          <div className="flex items-center gap-1.5">
            <span className="text-[13px] text-red-500">
              {connectionStatus === 'reconnecting' ? 'Reconnecting...' : 'Offline'}
            </span>
            <button
              onClick={onReconnect}
              className="flex items-center gap-1 text-[11px] text-blue-500 hover:text-blue-400 px-1.5 py-0.5 rounded hover:bg-blue-500/10 transition-colors"
              title="Reconnect now"
            >
              <RefreshCw className="w-3 h-3" />
              Reconnect
            </button>
          </div>
        ) : (
          <span className="text-[13px] text-gray-700 tabular-nums">
            {stats.totalSessions} session{stats.totalSessions !== 1 ? 's' : ''}
            {stats.busyCount > 0 && <span className="text-yellow-600 ml-1.5">{stats.busyCount} busy</span>}
            {stats.totalCost > 0 && <span className="text-gray-400 ml-1.5">${stats.totalCost.toFixed(2)}</span>}
          </span>
        )}
      </div>

      {/* Server info */}
      <div className="flex items-center gap-2 px-3 py-[3px] text-[11px] text-gray-400">
        <Server className="w-3 h-3 shrink-0" />
        <span className={serverStatus?.running ? 'text-green-600' : 'text-red-500'}>
          {sourceLabel}
        </span>
        <span>:{serverStatus?.port ?? 3099}</span>
        {health?.uptime && <span className="ml-auto">{health.uptime}</span>}
      </div>

      <Separator />

      {/* Sessions */}
      <div className="flex-1 overflow-y-auto overflow-x-hidden">
        {sessions.length === 0 ? (
          <div className="px-3 py-3 text-[13px] text-gray-400 text-center">
            {isOnline ? 'No active sessions' : 'Server disconnected'}
          </div>
        ) : (
          sessions.map(session => (
            <SessionItem key={session.id} session={session} />
          ))
        )}
      </div>

      <Separator />

      {/* Actions */}
      <MenuItem icon={Pin} label="Pin Float Window" onClick={handlePinFloat} />
      <div className="menu-item flex items-center gap-2 px-3 py-[5px]">
        <Eye className="w-4 h-4 text-gray-500 shrink-0" />
        <span className="text-[13px] text-gray-800">Opacity</span>
        <input
          type="range"
          min="10"
          max="100"
          value={Math.round(floatOpacity * 100)}
          onChange={e => setFloatOpacity(parseInt(e.target.value) / 100)}
          className="flex-1 h-1 accent-gray-500 cursor-default"
        />
        <span className="text-[11px] text-gray-400 tabular-nums w-7 text-right">{Math.round(floatOpacity * 100)}%</span>
      </div>
      <MenuItem icon={Globe} label="Open Dashboard" onClick={handleOpenDashboard} />

      <Separator />

      {/* Token */}
      {!editingToken ? (
        <MenuItem icon={Key} label="Auth Token" onClick={handleEditToken} />
      ) : (
        <div className="px-3 py-[5px]">
          <div className="flex items-center gap-1">
            <Key className="w-4 h-4 text-gray-500 shrink-0" />
            <input
              type="text"
              value={tokenValue}
              onChange={e => setTokenValue(e.target.value)}
              onKeyDown={e => {
                if (e.key === 'Enter') handleSaveToken();
                if (e.key === 'Escape') handleCancelToken();
              }}
              placeholder="Enter token..."
              autoFocus
              className="flex-1 text-[12px] bg-black/5 rounded px-1.5 py-0.5 outline-none focus:ring-1 focus:ring-blue-400 min-w-0 font-mono"
            />
            {tokenSaved ? (
              <Check className="w-4 h-4 text-green-500 shrink-0" />
            ) : (
              <>
                <button onClick={() => { navigator.clipboard.writeText(tokenValue); }} className="p-0.5 hover:bg-black/5 rounded" title="Copy token">
                  <Copy className="w-3.5 h-3.5 text-gray-400" />
                </button>
                <button onClick={handleRandomizeToken} className="p-0.5 hover:bg-black/5 rounded" title="Generate random token">
                  <RefreshCw className="w-3.5 h-3.5 text-orange-400" />
                </button>
                <button onClick={handleSaveToken} className="p-0.5 hover:bg-black/5 rounded" title="Save">
                  <Check className="w-3.5 h-3.5 text-blue-500" />
                </button>
                <button onClick={handleCancelToken} className="p-0.5 hover:bg-black/5 rounded" title="Cancel">
                  <X className="w-3.5 h-3.5 text-gray-400" />
                </button>
              </>
            )}
          </div>
        </div>
      )}

      {/* Bottom */}
      <MenuItem icon={LogOut} label="Disconnect" onClick={handleLogout} danger />
      <MenuItem icon={Power} label="Quit Agent Tracker" onClick={() => invoke('quit_app')} danger />
    </div>
  );
};
