import React, { useState, useEffect } from 'react';
import { Monitor, Pin, Globe, LogOut, Power, ChevronRight, Eye } from 'lucide-react';
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

const WindowItem: React.FC<{ win: AgentWindow }> = ({ win }) => {
  const color = STATUS_COLOR[win.status];
  return (
    <>
      <div className="menu-row flex items-center pl-[40px] pr-3 py-[3px]">
        <span className={`text-[7px] ${color} mr-2`}>{'\u25CF'}</span>
        <span className="text-[13px] text-gray-800 truncate flex-1">{win.name}</span>
        <span className={`text-[11px] ${color} ml-2`}>{STATUS_LABEL[win.status]}</span>
      </div>
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
        <WindowItem key={win.id} win={win} />
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

export const MenuBarPanel: React.FC<MenuBarPanelProps> = ({ sessions, connectionStatus, stats, onLogout }) => {
  const isOnline = connectionStatus === 'connected';
  const [floatOpacity, setFloatOpacity] = useState(() => {
    const saved = localStorage.getItem(OPACITY_KEY);
    return saved ? parseFloat(saved) : 1.0;
  });

  useEffect(() => {
    localStorage.setItem(OPACITY_KEY, String(floatOpacity));
    invoke('set_float_opacity', { opacity: floatOpacity }).catch(() => {});
  }, [floatOpacity]);

  const handlePinFloat = async () => {
    try { await invoke('show_float'); } catch (e) { console.error('show_float failed:', e); }
    // Apply stored opacity after showing
    setTimeout(() => {
      invoke('set_float_opacity', { opacity: floatOpacity }).catch(() => {});
    }, 100);
  };

  const handleOpenDashboard = () => {
    invoke('open_url', { url: 'http://localhost:3099' }).catch(console.error);
  };

  const handleLogout = async () => {
    await clearAuthToken();
    onLogout?.();
  };

  return (
    <div className="flex flex-col h-full select-none rounded-[10px] overflow-hidden py-1">
      {/* Stats header */}
      <div className="flex items-center gap-2 px-3 py-[5px]">
        <span className={`text-[8px] ${isOnline ? 'text-green-500' : 'text-red-500'}`}>{'\u25CF'}</span>
        {!isOnline ? (
          <span className="text-[13px] text-red-500">
            {connectionStatus === 'reconnecting' ? 'Reconnecting...' : 'Offline'}
          </span>
        ) : (
          <span className="text-[13px] text-gray-700 tabular-nums">
            {stats.totalSessions} session{stats.totalSessions !== 1 ? 's' : ''}
            {stats.busyCount > 0 && <span className="text-yellow-600 ml-1.5">{stats.busyCount} busy</span>}
            {stats.totalCost > 0 && <span className="text-gray-400 ml-1.5">${stats.totalCost.toFixed(2)}</span>}
          </span>
        )}
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

      {/* Bottom */}
      <MenuItem icon={LogOut} label="Disconnect" onClick={handleLogout} danger />
      <MenuItem icon={Power} label="Quit Agent Tracker" onClick={() => invoke('quit_app')} danger />
    </div>
  );
};
