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
  const statusColor = !isOnline ? 'text-red-500' : stats.busyCount > 0 ? 'text-yellow-500' : 'text-green-500';

  return (
    <div
      className="flex items-center gap-2 px-3 py-1.5 cursor-move select-none h-full rounded-lg overflow-hidden"
      data-tauri-drag-region
    >
      <Hexagon className={`w-3.5 h-3.5 ${statusColor} shrink-0`} />
      {isOnline ? (
        <span className="text-[11px] text-gray-700 whitespace-nowrap">
          {stats.totalSessions} session{stats.totalSessions !== 1 ? 's' : ''}
          {stats.busyCount > 0 && <span className="text-yellow-600"> &middot; {stats.busyCount} busy</span>}
          {stats.totalCost > 0 && <span className="text-gray-400"> &middot; ${stats.totalCost.toFixed(2)}</span>}
        </span>
      ) : (
        <span className="text-[11px] text-red-500 whitespace-nowrap">
          {connectionStatus === 'reconnecting' ? 'Reconnecting...' : 'Offline'}
        </span>
      )}
    </div>
  );
};
