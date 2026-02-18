import React, { useEffect, useState, useCallback } from 'react';
import { X, ChevronDown, ChevronUp } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { AgentSession, AgentWindow } from './shared/types';

interface FloatingBarProps {
  sessions: AgentSession[];
  stats: {
    totalSessions: number;
    busyCount: number;
    totalCost: number;
  };
  connectionStatus: 'connected' | 'reconnecting' | 'offline';
}

const DOT_COLOR: Record<string, string> = {
  BUSY: 'bg-yellow-500',
  PAUSED: 'bg-orange-500',
  IDLE: 'bg-green-500',
  COMPLETED: 'bg-cyan-500',
  OFFLINE: 'bg-gray-400',
};

const TEXT_COLOR: Record<string, string> = {
  BUSY: 'text-yellow-600',
  PAUSED: 'text-orange-500',
  IDLE: 'text-green-600',
  COMPLETED: 'text-cyan-600',
  OFFLINE: 'text-gray-400',
};

const COLLAPSED_HEIGHT = 52;
const BAR_WIDTH = 340;
const SESSION_ROW_HEIGHT = 22;
const WINDOW_ROW_HEIGHT = 18;
const EXPAND_PADDING = 8;

export const FloatingBar: React.FC<FloatingBarProps> = ({ sessions, stats, connectionStatus }) => {
  const [expanded, setExpanded] = useState(false);
  const isOnline = connectionStatus === 'connected';

  const busySession = sessions.find(s => s.status === 'BUSY');
  const busyWindow = busySession?.windows.find(w => w.status === 'BUSY');
  const busyTool = busyWindow?.claudeStatus?.current_tool || busyWindow?.claudeStatus?.action;

  const calcExpandedHeight = useCallback(() => {
    let h = COLLAPSED_HEIGHT + EXPAND_PADDING;
    for (const s of sessions) {
      h += SESSION_ROW_HEIGHT;
      h += s.windows.length * WINDOW_ROW_HEIGHT;
    }
    return Math.min(h, 400);
  }, [sessions]);

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    const height = expanded ? calcExpandedHeight() : COLLAPSED_HEIGHT;
    win.setSize(new LogicalSize(BAR_WIDTH, height)).catch(() => {});
  }, [expanded, calcExpandedHeight]);

  useEffect(() => {
    const onMouseDown = (e: MouseEvent) => {
      if ((e.target as HTMLElement).closest('[data-no-drag]')) return;
      getCurrentWebviewWindow().startDragging();
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, []);

  const handleClose = async () => {
    try { await invoke('hide_float'); } catch (e) { console.error(e); }
  };

  const toggleExpand = () => setExpanded(v => !v);

  return (
    <div className="flex flex-col h-full select-none overflow-hidden cursor-grab active:cursor-grabbing">
      {/* Compact bar (always visible) */}
      <div className="flex items-center shrink-0" style={{ height: COLLAPSED_HEIGHT }}>
        <div className="flex-1 min-w-0 px-3 py-1">
          {!isOnline ? (
            <span className="text-[11px] text-red-500">
              {connectionStatus === 'reconnecting' ? 'Reconnecting...' : 'Offline'}
            </span>
          ) : (
            <>
              <div className="flex items-center gap-1.5">
                <div className="flex items-center gap-[3px] shrink-0">
                  {sessions.map(s => (
                    <span
                      key={s.id}
                      className={`w-[6px] h-[6px] rounded-full ${DOT_COLOR[s.status] || 'bg-gray-400'}`}
                      title={s.name}
                    />
                  ))}
                </div>
                <span className="text-[11px] text-gray-700 truncate">
                  <span className="font-medium">{stats.totalSessions}</span> session{stats.totalSessions !== 1 ? 's' : ''}
                  {stats.busyCount > 0 && (
                    <span className="text-yellow-600"> · {stats.busyCount} busy</span>
                  )}
                  {stats.totalCost > 0 && (
                    <span className="text-gray-400"> · ${stats.totalCost.toFixed(2)}</span>
                  )}
                </span>
              </div>

              {busyWindow && (
                <div className="text-[10px] text-gray-500 truncate mt-0.5 pl-0.5">
                  <span className="text-gray-600">{busyWindow.name}</span>
                  {busyTool && (
                    <span className="text-orange-500"> → {busyTool}</span>
                  )}
                </div>
              )}
            </>
          )}
        </div>

        {/* Buttons */}
        <div className="flex items-center shrink-0 h-full">
          {isOnline && sessions.length > 0 && (
            <button
              data-no-drag
              onClick={toggleExpand}
              className="px-1.5 h-full flex items-center hover:bg-black/10 transition-colors cursor-default"
            >
              {expanded
                ? <ChevronUp className="w-3 h-3 text-gray-400" />
                : <ChevronDown className="w-3 h-3 text-gray-400" />
              }
            </button>
          )}
          <button
            data-no-drag
            onClick={handleClose}
            className="px-2 h-full flex items-center hover:bg-black/10 transition-colors cursor-default"
          >
            <X className="w-3 h-3 text-gray-400" />
          </button>
        </div>
      </div>

      {/* Expanded session list */}
      {expanded && isOnline && (
        <div className="flex-1 overflow-y-auto overflow-x-hidden px-2 pb-1.5" data-no-drag>
          <div className="border-t border-gray-200/60 mb-1" />
          {sessions.map(session => (
            <SessionRow key={session.id} session={session} />
          ))}
        </div>
      )}
    </div>
  );
};

const SessionRow: React.FC<{ session: AgentSession }> = ({ session }) => (
  <div className="mb-0.5">
    <div className="flex items-center gap-1.5 px-1 py-0.5">
      <span className={`w-[5px] h-[5px] rounded-full shrink-0 ${DOT_COLOR[session.status] || 'bg-gray-400'}`} />
      <span className="text-[11px] font-medium text-gray-700 truncate">{session.name}</span>
      <span className={`text-[9px] ml-auto shrink-0 ${TEXT_COLOR[session.status] || 'text-gray-400'}`}>
        {session.status}
      </span>
    </div>
    {session.windows.map(w => (
      <WindowRow key={w.id} window={w} />
    ))}
  </div>
);

const WindowRow: React.FC<{ window: AgentWindow }> = ({ window: w }) => {
  const tool = w.claudeStatus?.current_tool || w.claudeStatus?.action;
  return (
    <div className="flex items-center gap-1 pl-4 pr-1 py-[1px]">
      <span className={`w-[4px] h-[4px] rounded-full shrink-0 ${DOT_COLOR[w.status] || 'bg-gray-400'}`} />
      <span className="text-[10px] text-gray-500 truncate">{w.name}</span>
      {tool && (
        <span className="text-[9px] text-orange-500 truncate ml-auto shrink-0 max-w-[120px]">
          {tool}
        </span>
      )}
    </div>
  );
};
