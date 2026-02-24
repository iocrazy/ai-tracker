import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CommandPalette } from '../components/CommandPalette';
import type { ProjectInfo } from '../services/api';
import type { AgentSession, AppTab } from '../types';

// jsdom doesn't implement scrollIntoView
Element.prototype.scrollIntoView = vi.fn();

const mockProjects: ProjectInfo[] = [
  { git_dir: '/home/user/tracker', name: 'tracker', last_session: '', last_window: '', last_active_at: null, notes_count: 0, goals_count: 0, history_count: 10, description: '', status: 'active', tags: '', created_at: '', tech_stack: '', todos_count: 0 },
  { git_dir: '/home/user/api', name: 'api', last_session: '', last_window: '', last_active_at: null, notes_count: 0, goals_count: 0, history_count: 5, description: '', status: '', tags: '', created_at: '', tech_stack: '', todos_count: 0 },
];

const mockSessions: AgentSession[] = [
  {
    id: '$1', name: '1-tracker', status: 'BUSY', ip: '$1',
    gitDir: '/home/user/tracker',
    windows: [
      { id: '@1', name: 'main', windowIndex: 0, status: 'BUSY', lastActive: '5m', avatar: '' },
    ],
  },
];

const defaultProps = {
  isOpen: true,
  onClose: vi.fn(),
  projects: mockProjects,
  sessions: mockSessions,
  activeTab: 'WORKSTATIONS' as AppTab,
  onSwitchTab: vi.fn(),
  onOpenProject: vi.fn(),
  onStartSession: vi.fn(),
};

describe('CommandPalette', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders when open', () => {
    render(<CommandPalette {...defaultProps} />);
    expect(screen.getByPlaceholderText(/Search projects/i)).toBeInTheDocument();
  });

  it('does not render when closed', () => {
    render(<CommandPalette {...defaultProps} isOpen={false} />);
    expect(screen.queryByPlaceholderText(/Search projects/i)).not.toBeInTheDocument();
  });

  it('shows active projects by default', () => {
    render(<CommandPalette {...defaultProps} />);
    expect(screen.getByText('tracker')).toBeInTheDocument();
    expect(screen.getByText('ACTIVE')).toBeInTheDocument();
  });

  it('shows navigation items', () => {
    render(<CommandPalette {...defaultProps} />);
    // Should show tabs other than current (WORKSTATIONS)
    expect(screen.getByText('Go to Projects')).toBeInTheDocument();
    expect(screen.getByText('Go to Analytics')).toBeInTheDocument();
  });

  it('does not show current tab in navigation', () => {
    render(<CommandPalette {...defaultProps} />);
    expect(screen.queryByText('Go to Workstations')).not.toBeInTheDocument();
  });

  it('filters items by search query', async () => {
    const user = userEvent.setup();
    render(<CommandPalette {...defaultProps} />);
    const input = screen.getByPlaceholderText(/Search projects/i);
    await user.type(input, 'api');
    expect(screen.getByText('api')).toBeInTheDocument();
    // Tracker should not match 'api'
    expect(screen.queryByText('ACTIVE PROJECTS')).not.toBeInTheDocument();
  });

  it('shows no results message for unmatched query', async () => {
    const user = userEvent.setup();
    render(<CommandPalette {...defaultProps} />);
    const input = screen.getByPlaceholderText(/Search projects/i);
    await user.type(input, 'zzzznonexistent');
    expect(screen.getByText(/No results/)).toBeInTheDocument();
  });

  it('calls onClose on ESC key', async () => {
    const user = userEvent.setup();
    render(<CommandPalette {...defaultProps} />);
    const input = screen.getByPlaceholderText(/Search projects/i);
    await user.type(input, '{Escape}');
    expect(defaultProps.onClose).toHaveBeenCalled();
  });

  it('calls onClose when clicking backdrop', async () => {
    const user = userEvent.setup();
    render(<CommandPalette {...defaultProps} />);
    // Click the backdrop (outermost div)
    const backdrop = screen.getByPlaceholderText(/Search projects/i).closest('.fixed');
    if (backdrop) {
      await user.click(backdrop);
      expect(defaultProps.onClose).toHaveBeenCalled();
    }
  });
});
