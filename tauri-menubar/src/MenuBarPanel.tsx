import React, { useState } from 'react';
import { Monitor, ChevronDown, ChevronRight, Pin, LogOut } from 'lucide-react';
import { AgentSession, AgentWindow, ClaudeStatus } from './shared/types';
import { invoke } from '@tauri-apps/api/core';
import { clearAuthToken } from './shared/services/auth';

interface MenuBarPanelProps {
  sessions: AgentSession[];
  connectionStatus: 'connected' | 'reconnecting' | 'offline';
  stats: {
    totalSessions: number;
    busyCount: number;
    totalCost: number;
  };
  onLogout?: () => void;
}

const STATUS_DOT: Record<AgentWindow['status'], { color: string; label: string }> = {
  BUSY: { color: 'text-yellow-500', label: 'BUSY' },
  PAUSED: { color: 'text-orange-500', label: 'WAIT' },
  IDLE: { color: 'text-green-500', label: 'IDLE' },
  COMPLETED: { color: 'text-cyan-500', label: 'DONE' },
  OFFLINE: { color: 'text-gray-400', label: 'OFF' },
};

const ClaudeInfo: React.FC<{ status: ClaudeStatus }> = ({ status }) => (
  <div className="ml-7 mr-3 mb-1.5 px-2 py-1 bg-black/5 rounded text-[10px]">
    {(status.current_tool || status.action) && (
      <div className="text-orange-600 truncate mb-0.5">{status.current_tool || status.action}</div>
    )}
    <div className="flex items-center gap-1.5 text-gray-500">
      {status.model && <span>{status.model.replace('claude-', '').split('-')[0]}</span>}
      {status.cost != null && <span>${status.cost.toFixed(2)}</span>}
      {status.context_percent != null && (
        <span className={status.context_percent > 80 ? 'text-orange-500' : ''}>
          {status.context_percent.toFixed(0)}% ctx
        </span>
      )}
    </div>
  </div>
);

const WindowRow: React.FC<{ win: AgentWindow }> = ({ win }) => {
  const s = STATUS_DOT[win.status];
  return (
    <div>
      <div className="flex items-center gap-2 px-3 py-1 hover:bg-black/5 rounded-md mx-1">
        <span className={`text-[8px] ${s.color}`}>{'\u25CF'}</span>
        <span className="text-[12px] text-gray-800 truncate flex-1">{win.name}</span>
        <span className={`text-[10px] font-medium ${s.color}`}>{s.label}</span>
        {win.lastActive !== '--:--' && (
          <span className="text-[10px] text-gray-400 tabular-nums">{win.lastActive}</span>
        )}
      </div>
      {win.claudeStatus && (win.status === 'BUSY' || win.status === 'PAUSED') && (
        <ClaudeInfo status={win.claudeStatus} />
      )}
    </div>
  );
};

const SessionCard: React.FC<{ session: AgentSession }> = ({ session }) => {
  const [expanded, setExpanded] = useState(session.status === 'BUSY');
  const busyCount = session.windows.filter(w => w.status === 'BUSY').length;

  return (
    <div>
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-1.5 px-3 py-1.5 hover:bg-black/5 rounded-md mx-0 transition-colors"
      >
        {expanded
          ? <ChevronDown className="w-3 h-3 text-gray-400 shrink-0" />
          : <ChevronRight className="w-3 h-3 text-gray-400 shrink-0" />
        }
        <Monitor className="w-3 h-3 text-gray-500 shrink-0" />
        <span className="text-[12px] text-gray-800 font-medium truncate flex-1 text-left">{session.name}</span>
        {busyCount > 0 && (
          <span className="text-[10px] font-semibold text-yellow-600">{busyCount} BUSY</span>
        )}
      </button>
      {expanded && session.windows.map(win => (
        <WindowRow key={win.id} win={win} />
      ))}
    </div>
  );
};

export const MenuBarPanel: React.FC<MenuBarPanelProps> = ({ sessions, connectionStatus, stats, onLogout }) => {
  const isOnline = connectionStatus === 'connected';

  const handlePinFloat = async () => {
    try { await invoke('show_float'); } catch (e) { console.error('show_float failed:', e); }
  };

  const handleLogout = async () => {
    await clearAuthToken();
    onLogout?.();
  };

  return (
    <div className="flex flex-col h-full select-none rounded-[10px] overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2">
        <div className="flex items-center gap-1.5">
          <span className={`text-[8px] ${isOnline ? 'text-green-500' : 'text-red-500'}`}>{'\u25CF'}</span>
          <span className="text-[12px] font-semibold text-gray-700">
            {isOnline ? 'Agent Tracker is running' : connectionStatus === 'reconnecting' ? 'Reconnecting...' : 'Offline'}
          </span>
        </div>
      </div>

      <div className="mx-3 border-t border-black/10" />

      {/* Session list */}
      <div className="flex-1 overflow-y-auto py-1">
        {sessions.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-gray-400">
            <Monitor className="w-5 h-5 mb-1.5 opacity-50" />
            <span className="text-[11px]">{isOnline ? 'No active sessions' : 'Server disconnected'}</span>
          </div>
        ) : (
          <div className="space-y-0.5">
            {sessions.map(session => (
              <SessionCard key={session.id} session={session} />
            ))}
          </div>
        )}
      </div>

      <div className="mx-3 border-t border-black/10" />

      {/* Footer */}
      <div className="flex items-center justify-between px-3 py-2 text-[11px] text-gray-500">
        <div className="flex items-center gap-3">
          <button onClick={handlePinFloat} className="flex items-center gap-1 hover:text-gray-800 transition-colors">
            <Pin className="w-3 h-3" />
            <span>Float</span>
          </button>
          <button onClick={handleLogout} className="flex items-center gap-1 hover:text-gray-800 transition-colors">
            <LogOut className="w-3 h-3" />
          </button>
        </div>
        <div className="flex items-center gap-1.5 tabular-nums text-gray-400">
          {stats.busyCount > 0 && <span className="text-yellow-600">{stats.busyCount} busy</span>}
          <span>{stats.totalSessions} sessions</span>
          {stats.totalCost > 0 && <span>${stats.totalCost.toFixed(2)}</span>}
        </div>
      </div>
    </div>
  );
};
