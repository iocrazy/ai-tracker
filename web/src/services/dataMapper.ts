// Map backend data to frontend types

import { AgentSession, AgentWindow, TimelineEvent, ConsoleLog } from '../types';
import { BackendState, BackendTask, BackendHistoryRecord, TmuxWindowInfo, formatDuration, formatTime } from './api';

// Map task status to window status
// acknowledged=true means the task is done and confirmed, should show as IDLE
function mapTaskStatus(status: BackendTask['status'], acknowledged: boolean): AgentWindow['status'] {
  switch (status) {
    case 'in_progress': return 'BUSY';
    case 'awaiting_input': return 'PAUSED';
    case 'completed': return acknowledged ? 'IDLE' : 'COMPLETED';
    default: return 'IDLE';
  }
}

// Map tmux windows to AgentSessions (primary data source)
// Tasks are used to overlay status information
export function mapTmuxToSessions(
  tmuxWindows: TmuxWindowInfo[],
  tasks: BackendTask[]
): AgentSession[] {
  // Build task lookup by session_id + window_id
  const taskMap = new Map<string, BackendTask>();
  for (const task of tasks) {
    const key = `${task.session_id}|${task.window_id}`;
    // Keep the most recent task (or in_progress one)
    const existing = taskMap.get(key);
    if (!existing || task.status === 'in_progress' || task.status === 'awaiting_input') {
      taskMap.set(key, task);
    }
  }

  // Group tmux windows by session
  const sessionMap = new Map<string, { sessionName: string; windows: TmuxWindowInfo[]; gitDir?: string }>();
  for (const win of tmuxWindows) {
    const existing = sessionMap.get(win.session_id);
    if (existing) {
      existing.windows.push(win);
    } else {
      sessionMap.set(win.session_id, {
        sessionName: win.session_name,
        windows: [win],
        gitDir: win.git_dir,  // Get gitDir from first window
      });
    }
  }

  // Convert to AgentSession array
  const sessions: AgentSession[] = [];
  sessionMap.forEach((data, sessionId) => {
    // Map windows
    const windows: AgentWindow[] = data.windows.map(win => {
      const taskKey = `${win.session_id}|${win.window_id}`;
      const task = taskMap.get(taskKey);

      let status: AgentWindow['status'] = 'IDLE';
      let lastActive = '--:--';
      let summary = '';

      if (task) {
        status = mapTaskStatus(task.status, task.acknowledged);
        lastActive = task.started_at ? formatDuration(task.duration_seconds) : '--:--';
        summary = task.summary;
      }

      return {
        id: win.window_id,
        name: win.window_name,
        windowIndex: win.window_index,
        status,
        lastActive,
        // Use session_name + window_id for unique avatar
        avatar: `https://api.dicebear.com/7.x/avataaars/svg?seed=${encodeURIComponent(win.session_name + '-' + win.window_id)}`,
        summary,
        pane: String(win.pane_count),
      };
    });

    // Sort windows by tmux window index
    windows.sort((a, b) => a.windowIndex - b.windowIndex);

    // Determine session status
    const hasInProgress = windows.some(w => w.status === 'BUSY');
    const hasPaused = windows.some(w => w.status === 'PAUSED');
    const sessionStatus: AgentSession['status'] =
      hasInProgress ? 'BUSY' : hasPaused ? 'IDLE' : 'IDLE';

    sessions.push({
      id: sessionId,
      name: data.sessionName,
      status: sessionStatus,
      ip: sessionId,
      windows,
      gitDir: data.gitDir,
    });
  });

  // Sort sessions by name (numeric prefix first, then alphabetically)
  sessions.sort((a, b) => {
    // Extract leading number if present (e.g., "1-tracker" -> 1)
    const numA = parseInt(a.name.match(/^(\d+)/)?.[1] || '999', 10);
    const numB = parseInt(b.name.match(/^(\d+)/)?.[1] || '999', 10);
    if (numA !== numB) return numA - numB;
    return a.name.localeCompare(b.name);
  });

  return sessions;
}

// Legacy: Map backend tasks to frontend AgentSessions (fallback)
export function mapTasksToSessions(tasks: BackendTask[]): AgentSession[] {
  const sessionMap = new Map<string, { session: string; tasks: BackendTask[] }>();

  for (const task of tasks) {
    const existing = sessionMap.get(task.session_id);
    if (existing) {
      existing.tasks.push(task);
    } else {
      sessionMap.set(task.session_id, {
        session: task.session || task.session_id,
        tasks: [task],
      });
    }
  }

  const sessions: AgentSession[] = [];
  sessionMap.forEach((data, sessionId) => {
    const hasInProgress = data.tasks.some(t => t.status === 'in_progress');
    const hasPaused = data.tasks.some(t => t.status === 'awaiting_input');
    const sessionStatus: AgentSession['status'] =
      hasInProgress ? 'BUSY' : hasPaused ? 'IDLE' : 'IDLE';

    const windows: AgentWindow[] = data.tasks.map((task, idx) => ({
      id: `${task.window_id}-${task.pane}`,
      name: task.window || task.window_id,
      windowIndex: idx,
      status: mapTaskStatus(task.status, task.acknowledged),
      lastActive: task.started_at ? formatDuration(task.duration_seconds) : '--:--',
      avatar: `https://api.dicebear.com/7.x/avataaars/svg?seed=${encodeURIComponent(task.summary.slice(0, 10))}`,
      summary: task.summary,
      pane: task.pane,
    }));

    sessions.push({
      id: sessionId,
      name: data.session || sessionId.replace('$', 'S'),
      status: sessionStatus,
      ip: sessionId,
      windows,
    });
  });

  return sessions;
}

// Map backend history to frontend TimelineEvents
export function mapHistoryToTimeline(history: BackendHistoryRecord[]): TimelineEvent[] {
  return history.map((record, index) => {
    // Show session:window name like "agent:refactor" or fallback to IDs
    const sessionName = record.session || record.session_id.replace('$', 'S');
    const windowName = record.window || record.window_id.replace('@', 'W');
    const displayName = `${sessionName}:${windowName}`;

    return {
      id: String(record.id || index),
      time: formatTime(record.completed_at),
      user: displayName,
      action: 'COMPLETED',
      description: record.summary || '(no summary)',
      status: 'COMPLETED' as const,
      linkText: record.completion_note ? `→ ${record.completion_note.slice(0, 30)}...` : '→ 完成',
    };
  });
}

// Generate console logs from state (system status only)
export function generateConsoleLogs(state: BackendState): ConsoleLog[] {
  const logs: ConsoleLog[] = [
    { id: 'sys-1', type: 'system', text: '> Connected to tracker-server' },
    { id: 'sys-2', type: 'system', text: `> Active sessions: ${new Set(state.tasks.map(t => t.session_id)).size}` },
    { id: 'sys-3', type: 'system', text: '> Ready to execute tmux commands' },
    { id: 'sys-4', type: 'output', text: '> _' },
  ];

  return logs;
}
