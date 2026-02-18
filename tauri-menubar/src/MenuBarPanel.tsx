import React, { useState } from 'react';
import { Monitor, ChevronDown, ChevronRight, Activity, Pause, Check, PowerOff, Pin, Wifi, WifiOff } from 'lucide-react';
import { AgentSession, AgentWindow, ClaudeStatus } from './shared/types';
import { invoke } from '@tauri-apps/api/core';

interface MenuBarPanelProps {
  sessions: AgentSession[];
  connectionStatus: 'connected' | 'reconnecting' | 'offline';
  stats: {
    totalSessions: number;
    busyCount: number;
    totalCost: number;
  };
}

const STATUS_CONFIG: Record<AgentWindow['status'], { color: string; icon: React.ElementType; label: string }> = {
  IDLE: { color: 'text-green-500', icon: Activity, label: 'IDLE' },
  BUSY: { color: 'text-yellow-400', icon: Activity, label: 'BUSY' },
  PAUSED: { color: 'text-orange-400', icon: Pause, label: 'PAUSED' },
  COMPLETED: { color: 'text-cyan-400', icon: Check, label: 'DONE' },
  OFFLINE: { color: 'text-red-500', icon: PowerOff, label: 'OFF' },
};

const ClaudeInfo: React.FC<{ status: ClaudeStatus }> = ({ status }) => (
  <div className="pl-7 pb-1.5 space-y-0.5">
    {status.current_tool && (
      <div className="text-[11px] text-yellow-400/80 truncate">{status.current_tool}</div>
    )}
    {status.action && !status.current_tool && (
      <div className="text-[11px] text-yellow-400/80 truncate">{status.action}</div>
    )}
    <div className="flex items-center gap-2 text-[10px] text-neutral-500">
      {status.model && <span>{status.model.replace('claude-', '').split('-')[0]}</span>}
      {status.cost != null && <span>${status.cost.toFixed(2)}</span>}
      {status.context_percent != null && <span>{status.context_percent.toFixed(0)}%</span>}
      {status.session_duration && <span>{status.session_duration}</span>}
    </div>
  </div>
);

const WindowRow: React.FC<{ win: AgentWindow }> = ({ win }) => {
  const config = STATUS_CONFIG[win.status];
  return (
    <div className="py-1">
      <div className="flex items-center gap-2 px-3 py-0.5">
        <span className={`text-[10px] ${config.color}`}>{win.status === 'BUSY' ? '\u25CF' : win.status === 'IDLE' ? '\u25CB' : '\u25CE'}</span>
        <span className="text-[11px] text-neutral-300 truncate flex-1">{win.name}</span>
        <span className={`text-[10px] ${config.color}`}>{config.label}</span>
        {win.lastActive !== '--:--' && (
          <span className="text-[10px] text-neutral-600">{win.lastActive}</span>
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
  const sessionColor = busyCount > 0 ? 'text-yellow-400' : 'text-green-500';

  return (
    <div className="border-b border-neutral-800 last:border-b-0">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-2 px-3 py-2 hover:bg-neutral-800/50 transition-colors"
      >
        {expanded ? <ChevronDown className="w-3 h-3 text-neutral-500 shrink-0" /> : <ChevronRight className="w-3 h-3 text-neutral-500 shrink-0" />}
        <Monitor className="w-3.5 h-3.5 text-neutral-400 shrink-0" />
        <span className="text-xs text-neutral-200 font-medium truncate flex-1 text-left">{session.name}</span>
        <span className={`text-[10px] font-medium ${sessionColor}`}>
          {busyCount > 0 ? `${busyCount} BUSY` : 'IDLE'}
        </span>
      </button>
      {expanded && (
        <div className="pb-1">
          {session.windows.map(win => (
            <WindowRow key={win.id} win={win} />
          ))}
        </div>
      )}
    </div>
  );
};

export const MenuBarPanel: React.FC<MenuBarPanelProps> = ({ sessions, connectionStatus, stats }) => {
  const isOnline = connectionStatus === 'connected';

  const handlePinFloat = async () => {
    try {
      await invoke('show_float');
    } catch (e) {
      console.error('Failed to show float:', e);
    }
  };

  return (
    <div className="flex flex-col h-full bg-neutral-900/95 backdrop-blur-xl rounded-lg overflow-hidden border border-neutral-700/50">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-neutral-800">
        <span className="text-xs font-semibold text-neutral-300">Agent Tracker</span>
        <div className="flex items-center gap-1.5">
          {isOnline ? (
            <Wifi className="w-3 h-3 text-green-500" />
          ) : (
            <WifiOff className="w-3 h-3 text-red-500" />
          )}
          <span className={`text-[10px] font-medium ${isOnline ? 'text-green-500' : 'text-red-500'}`}>
            {isOnline ? 'ONLINE' : connectionStatus === 'reconnecting' ? 'RETRY' : 'OFFLINE'}
          </span>
        </div>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-neutral-600">
            <Monitor className="w-6 h-6 mb-2" />
            <span className="text-xs">{isOnline ? 'No sessions' : 'Disconnected'}</span>
          </div>
        ) : (
          sessions.map(session => (
            <SessionCard key={session.id} session={session} />
          ))
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between px-3 py-1.5 border-t border-neutral-800 bg-neutral-900">
        <button
          onClick={handlePinFloat}
          className="flex items-center gap-1 text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors"
        >
          <Pin className="w-3 h-3" />
          <span>Pin Window</span>
        </button>
        <div className="flex items-center gap-2 text-[10px] text-neutral-600">
          <span>{stats.totalSessions} sessions</span>
          {stats.totalCost > 0 && <span>${stats.totalCost.toFixed(2)}</span>}
        </div>
      </div>
    </div>
  );
};
