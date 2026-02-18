// Map backend data to frontend types — adapted from web/src/services/dataMapper.ts

import { AgentSession, AgentWindow } from '../types';
import { BackendTask } from './state';
import { TmuxWindowInfo } from './tmux';
import { formatDuration } from './helpers';

function mapTaskStatus(status: BackendTask['status'], acknowledged: boolean): AgentWindow['status'] {
  switch (status) {
    case 'in_progress': return 'BUSY';
    case 'awaiting_input': return 'PAUSED';
    case 'completed': return acknowledged ? 'IDLE' : 'COMPLETED';
    default: return 'IDLE';
  }
}

export function mapTmuxToSessions(
  tmuxWindows: TmuxWindowInfo[],
  tasks: BackendTask[]
): AgentSession[] {
  const taskMap = new Map<string, BackendTask>();
  for (const task of tasks) {
    const key = `${task.session_id}|${task.window_id}`;
    const existing = taskMap.get(key);
    if (!existing || task.status === 'in_progress' || task.status === 'awaiting_input') {
      taskMap.set(key, task);
    }
  }

  const sessionMap = new Map<string, { sessionName: string; windows: TmuxWindowInfo[]; gitDir?: string }>();
  for (const win of tmuxWindows) {
    const existing = sessionMap.get(win.session_id);
    if (existing) {
      existing.windows.push(win);
    } else {
      sessionMap.set(win.session_id, {
        sessionName: win.session_name,
        windows: [win],
        gitDir: win.git_dir,
      });
    }
  }

  const sessions: AgentSession[] = [];
  sessionMap.forEach((data, sessionId) => {
    const windows: AgentWindow[] = data.windows.map(win => {
      const taskKey = `${win.session_id}|${win.window_id}`;
      const task = taskMap.get(taskKey);

      let status: AgentWindow['status'] = 'IDLE';
      let lastActive = '--:--';

      if (task) {
        status = mapTaskStatus(task.status, task.acknowledged);
        lastActive = task.started_at ? formatDuration(task.duration_seconds) : '--:--';
      }

      return {
        id: win.window_id,
        name: win.window_name,
        status,
        lastActive,
        avatar: '',
        pane: String(win.pane_count),
      };
    });

    const hasInProgress = windows.some(w => w.status === 'BUSY');
    const sessionStatus: AgentSession['status'] = hasInProgress ? 'BUSY' : 'IDLE';

    sessions.push({
      id: sessionId,
      name: data.sessionName,
      status: sessionStatus,
      ip: sessionId,
      windows,
      gitDir: data.gitDir,
    });
  });

  sessions.sort((a, b) => {
    const numA = parseInt(a.name.match(/^(\d+)/)?.[1] || '999', 10);
    const numB = parseInt(b.name.match(/^(\d+)/)?.[1] || '999', 10);
    if (numA !== numB) return numA - numB;
    return a.name.localeCompare(b.name);
  });

  return sessions;
}
