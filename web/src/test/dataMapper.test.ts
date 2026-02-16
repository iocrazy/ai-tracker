import { mapTmuxToSessions, mapHistoryToTimeline, generateConsoleLogs } from '../services/dataMapper';
import type { BackendTask, BackendHistoryRecord, BackendState } from '../services/state';
import type { TmuxWindowInfo } from '../services/tmux';

// Minimal factory helpers
function makeTmuxWindow(overrides: Partial<TmuxWindowInfo> = {}): TmuxWindowInfo {
  return {
    session_id: '$1',
    session_name: '1-tracker',
    window_id: '@1',
    window_name: 'main',
    pane_count: 2,
    active: true,
    ...overrides,
  };
}

function makeTask(overrides: Partial<BackendTask> = {}): BackendTask {
  return {
    session_id: '$1',
    session: '1-tracker',
    window_id: '@1',
    window: 'main',
    pane: '1',
    status: 'in_progress',
    summary: 'Working on feature',
    completion_note: '',
    started_at: '2026-02-15T10:00:00Z',
    completed_at: null,
    duration_seconds: 120,
    acknowledged: false,
    archived: false,
    ...overrides,
  };
}

function makeHistory(overrides: Partial<BackendHistoryRecord> = {}): BackendHistoryRecord {
  return {
    id: 1,
    session_id: '$1',
    session: '1-tracker',
    window_id: '@1',
    window: 'main',
    pane: '1',
    summary: 'Completed feature X',
    completion_note: 'All tests pass',
    started_at: '2026-02-15T10:00:00Z',
    completed_at: '2026-02-15T10:05:00Z',
    duration_seconds: 300,
    ...overrides,
  };
}

function makeState(overrides: Partial<BackendState> = {}): BackendState {
  return {
    kind: 'state',
    tasks: [],
    archived_tasks: [],
    notes: [],
    archived: [],
    goals: [],
    history: [],
    message: '',
    ...overrides,
  };
}

describe('mapTmuxToSessions', () => {
  it('groups windows by session', () => {
    const windows = [
      makeTmuxWindow({ session_id: '$1', session_name: '1-tracker', window_id: '@1', window_name: 'main' }),
      makeTmuxWindow({ session_id: '$1', session_name: '1-tracker', window_id: '@2', window_name: 'dev' }),
      makeTmuxWindow({ session_id: '$2', session_name: '2-api', window_id: '@3', window_name: 'main' }),
    ];
    const result = mapTmuxToSessions(windows, []);
    expect(result).toHaveLength(2);
    expect(result[0].name).toBe('1-tracker');
    expect(result[0].windows).toHaveLength(2);
    expect(result[1].name).toBe('2-api');
    expect(result[1].windows).toHaveLength(1);
  });

  it('sorts sessions by numeric prefix', () => {
    const windows = [
      makeTmuxWindow({ session_id: '$3', session_name: '3-web' }),
      makeTmuxWindow({ session_id: '$1', session_name: '1-tracker' }),
      makeTmuxWindow({ session_id: '$2', session_name: '2-api' }),
    ];
    const result = mapTmuxToSessions(windows, []);
    expect(result.map(s => s.name)).toEqual(['1-tracker', '2-api', '3-web']);
  });

  it('overlays task status onto windows', () => {
    const windows = [makeTmuxWindow()];
    const tasks = [makeTask({ status: 'in_progress' })];
    const result = mapTmuxToSessions(windows, tasks);
    expect(result[0].windows[0].status).toBe('BUSY');
  });

  it('maps awaiting_input to PAUSED', () => {
    const windows = [makeTmuxWindow()];
    const tasks = [makeTask({ status: 'awaiting_input' })];
    const result = mapTmuxToSessions(windows, tasks);
    expect(result[0].windows[0].status).toBe('PAUSED');
  });

  it('maps completed+acknowledged to IDLE', () => {
    const windows = [makeTmuxWindow()];
    const tasks = [makeTask({ status: 'completed', acknowledged: true })];
    const result = mapTmuxToSessions(windows, tasks);
    expect(result[0].windows[0].status).toBe('IDLE');
  });

  it('maps completed+unacknowledged to COMPLETED', () => {
    const windows = [makeTmuxWindow()];
    const tasks = [makeTask({ status: 'completed', acknowledged: false })];
    const result = mapTmuxToSessions(windows, tasks);
    expect(result[0].windows[0].status).toBe('COMPLETED');
  });

  it('defaults to IDLE when no task matches', () => {
    const windows = [makeTmuxWindow()];
    const result = mapTmuxToSessions(windows, []);
    expect(result[0].windows[0].status).toBe('IDLE');
  });

  it('returns empty array for no windows', () => {
    expect(mapTmuxToSessions([], [])).toEqual([]);
  });

  it('carries gitDir from tmux window', () => {
    const windows = [makeTmuxWindow({ git_dir: '/home/user/project' })];
    const result = mapTmuxToSessions(windows, []);
    expect(result[0].gitDir).toBe('/home/user/project');
  });
});

describe('mapHistoryToTimeline', () => {
  it('maps history records to timeline events', () => {
    const records = [makeHistory()];
    const result = mapHistoryToTimeline(records);
    expect(result).toHaveLength(1);
    expect(result[0].user).toBe('1-tracker:main');
    expect(result[0].action).toBe('COMPLETED');
    expect(result[0].description).toBe('Completed feature X');
  });

  it('handles missing summary', () => {
    const records = [makeHistory({ summary: '' })];
    const result = mapHistoryToTimeline(records);
    expect(result[0].description).toBe('(no summary)');
  });

  it('returns empty array for empty input', () => {
    expect(mapHistoryToTimeline([])).toEqual([]);
  });

  it('uses index as fallback id', () => {
    const records = [makeHistory({ id: 0 })];
    const result = mapHistoryToTimeline(records);
    // id 0 is falsy, so falls back to index (0) -> "0"
    expect(result[0].id).toBe('0');
  });
});

describe('generateConsoleLogs', () => {
  it('generates system logs', () => {
    const state = makeState();
    const logs = generateConsoleLogs(state);
    expect(logs.length).toBeGreaterThanOrEqual(3);
    expect(logs[0].type).toBe('system');
    expect(logs[0].text).toContain('Connected');
  });

  it('counts unique sessions', () => {
    const state = makeState({
      tasks: [
        makeTask({ session_id: '$1' }),
        makeTask({ session_id: '$1', window_id: '@2' }),
        makeTask({ session_id: '$2' }),
      ],
    });
    const logs = generateConsoleLogs(state);
    const sessionLog = logs.find(l => l.text.includes('Active sessions'));
    expect(sessionLog?.text).toContain('2');
  });
});
