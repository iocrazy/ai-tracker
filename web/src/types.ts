
export type AppTab = 'WORKSTATIONS' | 'PROJECTS' | 'ANALYTICS' | 'CONSOLE' | 'SETTINGS';

export interface ClaudeStatus {
    agent_type: 'claude' | 'opencode' | null;  // Detected AI agent type
    action: string | null;
    current_tool: string | null;
    model: string | null;
    context_percent: number | null;
    tokens: number | null;
    cost: number | null;
    session_duration: string | null;
    pane: string | null;  // Detected pane where Claude runs
}

export interface AgentWindow {
    id: string;
    name: string; // e.g. "2.1.29"
    windowIndex: number;  // tmux window index for ordering/swap
    status: 'IDLE' | 'BUSY' | 'OFFLINE' | 'PAUSED' | 'COMPLETED';
    lastActive: string;
    avatar: string;
    claudeStatus?: ClaudeStatus;
    claudePane?: string;  // Pane number where Claude runs (default: "1")
}

export interface AgentSession {
    id: string;
    name: string;
    status: 'IDLE' | 'BUSY' | 'OFFLINE';
    ip: string;
    windows: AgentWindow[];
    gitDir?: string;  // Git directory for the session
}

export interface TimelineEvent {
    id: string;
    time: string;
    user: string;
    action: string;
    description: string;
    status?: 'COMPLETED' | 'PENDING' | 'FAILED';
    linkText?: string;
    // Enhanced fields from history API
    historyId?: number;
    filePath?: string;  // Session JSONL file path (for session-based entries)
    messageCount?: number;
    duration?: number;
    // Grouped view fields
    groupIds?: number[];  // history IDs in this group (for grouped detail)
    taskCount?: number;
}

export interface ConsoleLog {
    id: string;
    type: 'output' | 'input' | 'system';
    text: string;
}

export interface AppSettings {
    theme: 'PHOSPHOR_GREEN' | 'AMBER' | 'CYAN' | 'MODERN';
    scanlines: boolean;
    flicker: boolean;
    glow: boolean;
    noise: boolean;
    rgbShift: boolean;
    perspectiveGrid: boolean;
}

export interface SystemMetric {
    id: string;
    label: string;
    value: number;
    max: number;
    unit: string;
    status: 'NORMAL' | 'WARNING' | 'CRITICAL';
    history: number[];
}

export interface LogEntry {
    id: string;
    timestamp: string;
    level: 'INFO' | 'WARN' | 'ERROR' | 'SYS';
    message: string;
}

export interface ConsoleTarget {
    session: string;
    window: string;      // window name for display
    windowId: string;    // window ID (e.g., @9) for tmux targeting
}
