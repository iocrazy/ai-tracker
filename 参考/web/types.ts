
export type AppTab = 'WORKSTATIONS' | 'TIMELINE' | 'CONSOLE' | 'SETTINGS';

export interface AgentWindow {
    id: string;
    name: string; // e.g. "2.1.29"
    status: 'IDLE' | 'BUSY' | 'OFFLINE' | 'PAUSED' | 'COMPLETED';
    lastActive: string;
    avatar: string;
}

export interface AgentSession {
    id: string;
    name: string;
    status: 'IDLE' | 'BUSY' | 'OFFLINE';
    ip: string;
    windows: AgentWindow[];
}

export interface TimelineEvent {
    id: string;
    time: string;
    user: string;
    action: string;
    description: string;
    status: 'COMPLETED' | 'PENDING' | 'FAILED';
    linkText: string;
}

export interface ConsoleLog {
    id: string;
    type: 'output' | 'input' | 'system';
    text: string;
}

export interface AppSettings {
    theme: 'PHOSPHOR_GREEN' | 'AMBER' | 'CYAN';
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
    window: string;
}
