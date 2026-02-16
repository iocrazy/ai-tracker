// State management, WebSocket, and stream APIs

import { API_BASE, WS_BASE, authFetch, getAuthToken } from './auth';
import type { ChatMessageEvent } from './claude';
import type { TmuxWindowInfo } from './tmux';

// Backend types (from tracker-server)
export interface BackendTask {
  session_id: string;
  session: string;
  window_id: string;
  window: string;
  pane: string;
  status: 'in_progress' | 'awaiting_input' | 'completed';
  summary: string;
  completion_note: string;
  started_at: string | null;
  completed_at: string | null;
  duration_seconds: number;
  acknowledged: boolean;
  archived: boolean;
}

export interface BackendNote {
  id: string;
  scope: 'window' | 'session' | 'all';
  session_id: string;
  session: string;
  window_id: string;
  window: string;
  pane: string;
  summary: string;
  completed: boolean;
  archived: boolean;
}

export interface BackendGoal {
  id: string;
  summary: string;
  completed: boolean;
}

export interface BackendHistoryRecord {
  id: number;
  session_id: string;
  session: string;
  window_id: string;
  window: string;
  pane: string;
  summary: string;
  completion_note: string;
  started_at: string | null;
  completed_at: string | null;
  duration_seconds: number;
}

export interface BackendState {
  kind: string;
  tasks: BackendTask[];
  archived_tasks: BackendTask[];
  notes: BackendNote[];
  archived: BackendNote[];
  goals: BackendGoal[];
  history: BackendHistoryRecord[];
  message: string;
  changed?: string[];  // Tables that changed (for selective re-rendering)
}

// Fetch current state
export async function fetchState(): Promise<BackendState> {
  const response = await authFetch(`${API_BASE}/state`);
  if (!response.ok) {
    throw new Error(`Failed to fetch state: ${response.status}`);
  }
  return response.json();
}

// Send a command
export async function sendCommand(command: string, params: Record<string, string> = {}): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, ...params }),
  });
  return response.json();
}

// Realtime message from WebSocket (state + tmux windows)
export interface RealtimeMessage {
  state: BackendState;
  tmux_windows: TmuxWindowInfo[];
}

// Stream chunk from real-time pane output
export interface StreamChunk {
  pane_id: string;
  target: string;
  raw: string;
  text: string;
  timestamp: string;
}

// Stream message from WebSocket
export interface StreamMessage {
  kind: 'stream';
  chunk: StreamChunk;
}

// WebSocket callbacks
export type ConnectionStatus = 'connected' | 'reconnecting' | 'offline';

interface WebSocketCallbacks {
  onStateUpdate: (msg: RealtimeMessage) => void;
  onStreamChunk?: (chunk: StreamChunk) => void;
  onChatMessage?: (event: ChatMessageEvent) => void;
  onConnectionChange?: (status: ConnectionStatus, retryCount?: number) => void;
}

// Reconnection state (module-level so it persists across calls)
let _retryCount = 0;
let _reconnectTimer: ReturnType<typeof setTimeout> | null = null;
const BASE_DELAY = 1000;
const MAX_DELAY = 30000;

export function connectWebSocket(
  callbacksOrOnMessage: WebSocketCallbacks | ((msg: RealtimeMessage) => void)
): WebSocket {
  // Clear any pending reconnect timer
  if (_reconnectTimer) {
    clearTimeout(_reconnectTimer);
    _reconnectTimer = null;
  }

  const token = getAuthToken();
  const wsUrl = token ? `${WS_BASE}?token=${encodeURIComponent(token)}` : WS_BASE;
  const ws = new WebSocket(wsUrl);

  // Support both old and new callback formats
  const callbacks: WebSocketCallbacks = typeof callbacksOrOnMessage === 'function'
    ? { onStateUpdate: callbacksOrOnMessage }
    : callbacksOrOnMessage;

  ws.onopen = () => {
    console.log('[WS] Connected to tracker-server');
    _retryCount = 0;
    callbacks.onConnectionChange?.('connected');
  };

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);

      // Handle stream messages
      if (data.kind === 'stream' && callbacks.onStreamChunk) {
        callbacks.onStreamChunk(data.chunk as StreamChunk);
        return;
      }

      // Handle chat message events
      if (data.kind === 'chat' && callbacks.onChatMessage) {
        callbacks.onChatMessage(data as ChatMessageEvent);
        return;
      }

      // Handle state messages (new format: state + tmux_windows)
      if (data.state && data.tmux_windows) {
        callbacks.onStateUpdate(data as RealtimeMessage);
      } else if (data.kind === 'state') {
        // Legacy format compatibility
        callbacks.onStateUpdate({
          state: data as BackendState,
          tmux_windows: [],
        });
      }
    } catch (e) {
      console.error('[WS] Failed to parse message:', e);
    }
  };

  ws.onclose = () => {
    _retryCount++;
    const delay = Math.min(BASE_DELAY * Math.pow(2, _retryCount - 1), MAX_DELAY) + Math.random() * 1000;
    console.log(`[WS] Disconnected, reconnecting in ${(delay / 1000).toFixed(1)}s (attempt #${_retryCount})...`);
    callbacks.onConnectionChange?.('reconnecting', _retryCount);
    _reconnectTimer = setTimeout(() => connectWebSocket(callbacks), delay);
  };

  ws.onerror = (error) => {
    console.error('[WS] Error:', error);
  };

  return ws;
}

// Health check API (no auth required)
export async function fetchHealth(): Promise<{ status: string; checks: Record<string, any>; response_ms: number } | null> {
  try {
    const res = await fetch(`${API_BASE}/health`);
    if (!res.ok) return null;
    return res.json();
  } catch {
    return null;
  }
}

// Stream control APIs
export async function startStream(
  session: string,
  window: string,
  pane: string
): Promise<{ success: boolean; target: string; message: string }> {
  const response = await authFetch(`${API_BASE}/stream/start`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window, pane }),
  });
  return response.json();
}

export async function stopStream(pane: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/stream/stop`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pane }),
  });
  return response.json();
}

export interface StreamEntry {
  pane_id: string;
  target: string;
}

export async function listStreams(): Promise<StreamEntry[]> {
  const response = await authFetch(`${API_BASE}/stream/list`);
  const data = await response.json();
  return data.streams || [];
}

// Log viewer API
export interface LogEntry {
  timestamp: string;
  level: string;
  module: string;
  message: string;
}

export async function fetchLogs(params: { limit?: number; level?: string; search?: string } = {}): Promise<{ entries: LogEntry[]; total: number }> {
  const searchParams = new URLSearchParams();
  if (params.limit) searchParams.set('limit', String(params.limit));
  if (params.level) searchParams.set('level', params.level);
  if (params.search) searchParams.set('search', params.search);

  const response = await authFetch(`${API_BASE}/logs?${searchParams}`);
  if (!response.ok) return { entries: [], total: 0 };
  return response.json();
}
