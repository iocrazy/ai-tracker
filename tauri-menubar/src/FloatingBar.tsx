import React from 'react';
import { Hexagon } from 'lucide-react';

interface FloatingBarProps {
  sessions: { length: number };
  stats: {
    totalSessions: number;
    busyCount: number;
    totalCost: number;
  };
  connectionStatus: 'connected' | 'reconnecting' | 'offline';
}

export const FloatingBar: React.FC<FloatingBarProps> = ({ stats, connectionStatus }) => {
  const isOnline = connectionStatus === 'connected';
  const statusColor = !isOnline ? 'text-red-500' : stats.busyCount > 0 ? 'text-yellow-400' : 'text-green-500';

  return (
    <div
      className="flex items-center gap-2 px-3 py-1.5 bg-neutral-900/90 backdrop-blur-xl rounded-lg border border-neutral-700/50 cursor-move select-none"
      data-tauri-drag-region
    >
      <Hexagon className={`w-3.5 h-3.5 ${statusColor} shrink-0`} />
      {isOnline ? (
        <span className="text-[11px] text-neutral-300 whitespace-nowrap">
          {stats.totalSessions} session{stats.totalSessions !== 1 ? 's' : ''}
          {stats.busyCount > 0 && <span className="text-yellow-400"> &middot; {stats.busyCount} busy</span>}
          {stats.totalCost > 0 && <span className="text-neutral-500"> &middot; ${stats.totalCost.toFixed(2)}</span>}
        </span>
      ) : (
        <span className="text-[11px] text-red-400 whitespace-nowrap">
          {connectionStatus === 'reconnecting' ? 'Reconnecting...' : 'Offline'}
        </span>
      )}
    </div>
  );
};
