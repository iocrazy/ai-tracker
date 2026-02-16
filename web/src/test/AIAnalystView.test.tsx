import { render, screen } from '@testing-library/react';
import { AIAnalystView } from '../components/AIAnalystView';
import type { BackendState, BackendHistoryRecord } from '../services/state';
import type { AgentSession, TimelineEvent } from '../types';

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

function makeHistory(overrides: Partial<BackendHistoryRecord> = {}): BackendHistoryRecord {
  return {
    id: 1,
    session_id: '$1',
    session: 'tracker',
    window_id: '@1',
    window: 'main',
    pane: '1',
    summary: 'Test task',
    completion_note: 'Done',
    started_at: '2026-02-15T10:00:00Z',
    completed_at: '2026-02-15T10:05:00Z',
    duration_seconds: 300,
    ...overrides,
  };
}

const emptySessions: AgentSession[] = [];
const emptyTimeline: TimelineEvent[] = [];

describe('AIAnalystView', () => {
  it('renders the heading', () => {
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={makeState()} />
    );
    expect(screen.getByText('SYSTEM ANALYTICS')).toBeInTheDocument();
  });

  it('shows zero stats when no data', () => {
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={makeState()} />
    );
    // Sessions: 0, Active: 0, In Progress: 0, Completed: 0
    const zeros = screen.getAllByText('0');
    expect(zeros.length).toBeGreaterThanOrEqual(4);
  });

  it('counts completed tasks from history', () => {
    const state = makeState({
      history: [
        makeHistory({ id: 1, duration_seconds: 120 }),
        makeHistory({ id: 2, duration_seconds: 180 }),
        makeHistory({ id: 3, duration_seconds: 240 }),
      ],
    });
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={state} />
    );
    expect(screen.getByText('3')).toBeInTheDocument(); // 3 completed
  });

  it('computes average duration', () => {
    const state = makeState({
      history: [
        makeHistory({ id: 1, duration_seconds: 60 }),
        makeHistory({ id: 2, duration_seconds: 120 }),
      ],
    });
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={state} />
    );
    // avg = (60 + 120) / 2 / 60 = 1.5m
    expect(screen.getByText('1.5m')).toBeInTheDocument();
  });

  it('shows completion rate section', () => {
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={makeState()} />
    );
    expect(screen.getByText('COMPLETION RATE')).toBeInTheDocument();
  });

  it('shows hourly activity section', () => {
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={makeState()} />
    );
    expect(screen.getByText('HOURLY ACTIVITY')).toBeInTheDocument();
  });

  it('shows session breakdown', () => {
    const state = makeState({
      history: [
        makeHistory({ session: 'tracker', duration_seconds: 300 }),
        makeHistory({ id: 2, session: 'tracker', duration_seconds: 600 }),
        makeHistory({ id: 3, session: 'api', duration_seconds: 120 }),
      ],
    });
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={state} />
    );
    expect(screen.getByText('tracker')).toBeInTheDocument();
    expect(screen.getByText('api')).toBeInTheDocument();
  });

  it('handles null backendState', () => {
    render(
      <AIAnalystView sessions={emptySessions} timeline={emptyTimeline} backendState={null} />
    );
    expect(screen.getByText('SYSTEM ANALYTICS')).toBeInTheDocument();
  });
});
