export interface ClaudeStatus {
    agent_type: 'claude' | 'opencode' | null;
    action: string | null;
    current_tool: string | null;
    model: string | null;
    context_percent: number | null;
    tokens: number | null;
    cost: number | null;
    session_duration: string | null;
    pane: string | null;
}

export interface AgentWindow {
    id: string;
    name: string;
    windowIndex: number;
    status: 'IDLE' | 'BUSY' | 'OFFLINE' | 'PAUSED' | 'COMPLETED';
    lastActive: string;
    avatar: string;
    claudeStatus?: ClaudeStatus;
    claudePane?: string;
}

export interface AgentSession {
    id: string;
    name: string;
    status: 'IDLE' | 'BUSY' | 'OFFLINE';
    ip: string;
    windows: AgentWindow[];
    gitDir?: string;
}
