import React, { useMemo } from 'react';
import { AgentSession, TimelineEvent } from '../types';
import { BackendState, BackendHistoryRecord } from '../services/state';
import { BarChart3, Activity, Clock, Zap, TrendingUp, Monitor } from 'lucide-react';

interface AIAnalystViewProps {
  sessions: AgentSession[];
  timeline: TimelineEvent[];
  backendState: BackendState | null;
}

interface ComputedStats {
  activeSessions: number;
  totalWindows: number;
  busyWindows: number;
  tasksInProgress: number;
  tasksCompleted: number;
  completionRate: number;
  avgDurationMin: number;
  totalDurationHrs: number;
  recentHistory: BackendHistoryRecord[];
  sessionBreakdown: { name: string; taskCount: number; totalDuration: number }[];
  hourlyActivity: number[];
}

function computeStats(
  sessions: AgentSession[],
  timeline: TimelineEvent[],
  state: BackendState | null
): ComputedStats {
  const activeSessions = sessions.filter(s => s.status !== 'OFFLINE').length;
  const totalWindows = sessions.reduce((sum, s) => sum + s.windows.length, 0);
  const busyWindows = sessions.reduce(
    (sum, s) => sum + s.windows.filter(w => w.status === 'BUSY').length,
    0
  );

  const tasks = state?.tasks ?? [];
  const archived = state?.archived_tasks ?? [];
  const history = state?.history ?? [];

  const tasksInProgress = tasks.filter(t => t.status === 'in_progress').length;
  const tasksCompleted = history.length + archived.filter(t => t.status === 'completed').length;
  const totalTasks = tasksInProgress + tasksCompleted;
  const completionRate = totalTasks > 0 ? (tasksCompleted / totalTasks) * 100 : 0;

  const durations = history.filter(h => h.duration_seconds > 0).map(h => h.duration_seconds);
  const avgDurationMin = durations.length > 0
    ? durations.reduce((a, b) => a + b, 0) / durations.length / 60
    : 0;
  const totalDurationHrs = durations.reduce((a, b) => a + b, 0) / 3600;

  const recentHistory = [...history]
    .sort((a, b) => (b.completed_at ?? '').localeCompare(a.completed_at ?? ''))
    .slice(0, 10);

  // Session breakdown
  const sessionMap = new Map<string, { taskCount: number; totalDuration: number }>();
  for (const h of history) {
    const existing = sessionMap.get(h.session) ?? { taskCount: 0, totalDuration: 0 };
    existing.taskCount++;
    existing.totalDuration += h.duration_seconds;
    sessionMap.set(h.session, existing);
  }
  const sessionBreakdown = [...sessionMap.entries()]
    .map(([name, data]) => ({ name, ...data }))
    .sort((a, b) => b.taskCount - a.taskCount)
    .slice(0, 8);

  // Hourly activity (24h histogram from completed_at)
  const hourlyActivity = new Array(24).fill(0);
  for (const h of history) {
    if (h.completed_at) {
      try {
        const hour = new Date(h.completed_at).getHours();
        hourlyActivity[hour]++;
      } catch { /* skip invalid dates */ }
    }
  }

  return {
    activeSessions,
    totalWindows,
    busyWindows,
    tasksInProgress,
    tasksCompleted,
    completionRate,
    avgDurationMin,
    totalDurationHrs,
    recentHistory,
    sessionBreakdown,
    hourlyActivity,
  };
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

export const AIAnalystView: React.FC<AIAnalystViewProps> = ({ sessions, timeline, backendState }) => {
  const stats = useMemo(
    () => computeStats(sessions, timeline, backendState),
    [sessions, timeline, backendState]
  );

  const maxHourly = Math.max(...stats.hourlyActivity, 1);

  return (
    <div className="flex flex-col gap-4 sm:gap-6 pt-4 pb-10 px-2 sm:px-0">
      <div className="flex items-center gap-4 sm:gap-6 mb-2">
        <h2 className="text-lg sm:text-2xl font-black text-green-700 uppercase tracking-tighter bg-green-900/10 px-3 sm:px-4 py-1 font-pixel">
          <BarChart3 className="w-5 h-5 inline mr-2" />SYSTEM ANALYTICS
        </h2>
      </div>

      {/* KPI Cards */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-3">
        <StatCard icon={Monitor} label="Sessions" value={String(stats.activeSessions)} sub={`${stats.totalWindows} windows`} />
        <StatCard icon={Zap} label="Active" value={String(stats.busyWindows)} sub="busy now" />
        <StatCard icon={Activity} label="In Progress" value={String(stats.tasksInProgress)} sub="tasks" />
        <StatCard icon={TrendingUp} label="Completed" value={String(stats.tasksCompleted)} sub="total" />
        <StatCard icon={Clock} label="Avg Duration" value={`${stats.avgDurationMin.toFixed(1)}m`} sub="per task" />
        <StatCard icon={BarChart3} label="Total Time" value={`${stats.totalDurationHrs.toFixed(1)}h`} sub="tracked" />
      </div>

      {/* Completion Rate Bar */}
      <div className="border-2 border-green-600 p-4 sm:p-6 relative">
        <h3 className="absolute -top-4 left-4 bg-[#050505] px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm uppercase">
          COMPLETION RATE
        </h3>
        <div className="mt-2">
          <div className="flex justify-between text-green-400 text-sm mb-2">
            <span>{stats.tasksCompleted} completed</span>
            <span>{stats.completionRate.toFixed(0)}%</span>
          </div>
          <div className="h-4 bg-green-900/30 border border-green-800 overflow-hidden">
            <div
              className="h-full bg-green-500 transition-all duration-500"
              style={{ width: `${Math.min(stats.completionRate, 100)}%` }}
            />
          </div>
        </div>
      </div>

      {/* Hourly Activity */}
      <div className="border-2 border-green-600 p-4 sm:p-6 relative">
        <h3 className="absolute -top-4 left-4 bg-[#050505] px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm uppercase">
          HOURLY ACTIVITY
        </h3>
        <div className="mt-2 flex items-end gap-[2px] h-20">
          {stats.hourlyActivity.map((count, hour) => (
            <div key={hour} className="flex-1 flex flex-col items-center group relative">
              <div
                className="w-full bg-green-600 hover:bg-green-400 transition-colors min-h-[2px]"
                style={{ height: `${(count / maxHourly) * 100}%` }}
                title={`${hour}:00 — ${count} tasks`}
              />
            </div>
          ))}
        </div>
        <div className="flex justify-between text-green-800 text-[10px] mt-1 font-mono">
          <span>00</span><span>06</span><span>12</span><span>18</span><span>23</span>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 sm:gap-6">
        {/* Session Breakdown */}
        <div className="border-2 border-green-600 p-4 sm:p-6 relative">
          <h3 className="absolute -top-4 left-4 bg-[#050505] px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm uppercase">
            SESSION BREAKDOWN
          </h3>
          <div className="mt-2 space-y-2">
            {stats.sessionBreakdown.length === 0 ? (
              <div className="text-green-800 text-sm">No session data yet.</div>
            ) : (
              stats.sessionBreakdown.map(s => {
                const maxTasks = stats.sessionBreakdown[0]?.taskCount ?? 1;
                return (
                  <div key={s.name} className="flex items-center gap-3">
                    <span className="text-green-400 font-mono text-sm w-28 truncate">{s.name}</span>
                    <div className="flex-1 h-3 bg-green-900/30 border border-green-800 overflow-hidden">
                      <div
                        className="h-full bg-green-600"
                        style={{ width: `${(s.taskCount / maxTasks) * 100}%` }}
                      />
                    </div>
                    <span className="text-green-500 text-xs font-mono w-16 text-right">
                      {s.taskCount}t / {formatDuration(s.totalDuration)}
                    </span>
                  </div>
                );
              })
            )}
          </div>
        </div>

        {/* Recent Completions */}
        <div className="border-2 border-green-600 p-4 sm:p-6 relative">
          <h3 className="absolute -top-4 left-4 bg-[#050505] px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm uppercase">
            RECENT COMPLETIONS
          </h3>
          <div className="mt-2 space-y-1.5 max-h-64 overflow-y-auto">
            {stats.recentHistory.length === 0 ? (
              <div className="text-green-800 text-sm">No completed tasks yet.</div>
            ) : (
              stats.recentHistory.map(h => (
                <div key={h.id} className="flex items-start gap-2 py-1 border-b border-green-900/20">
                  <span className="text-green-700 text-[10px] font-mono shrink-0 pt-0.5">
                    {h.completed_at ? new Date(h.completed_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '--:--'}
                  </span>
                  <span className="text-green-400 text-xs leading-snug line-clamp-2">
                    {h.summary || h.completion_note || '(no summary)'}
                  </span>
                  <span className="text-green-700 text-[10px] font-mono ml-auto shrink-0">
                    {formatDuration(h.duration_seconds)}
                  </span>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
};

// Stat card sub-component
const StatCard: React.FC<{
  icon: React.ElementType;
  label: string;
  value: string;
  sub: string;
}> = ({ icon: Icon, label, value, sub }) => (
  <div className="border border-green-700/40 bg-green-900/10 p-3 text-center">
    <Icon className="w-4 h-4 text-green-600 mx-auto mb-1" />
    <div className="text-green-300 text-xl sm:text-2xl font-bold font-mono">{value}</div>
    <div className="text-green-500 text-[10px] uppercase tracking-wider">{label}</div>
    <div className="text-green-700 text-[10px]">{sub}</div>
  </div>
);
