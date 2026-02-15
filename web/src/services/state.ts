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
interface WebSocketCallbacks {
  onStateUpdate: (msg: RealtimeMessage) => void;
  onStreamChunk?: (chunk: StreamChunk) => void;
  onChatMessage?: (event: ChatMessageEvent) => void;
}

export function connectWebSocket(
  callbacksOrOnMessage: WebSocketCallbacks | ((msg: RealtimeMessage) => void)
): WebSocket {
  const token = getAuthToken();
  const wsUrl = token ? `${WS_BASE}?token=${encodeURIComponent(token)}` : WS_BASE;
  const ws = new WebSocket(wsUrl);

  // Support both old and new callback formats
  const callbacks: WebSocketCallbacks = typeof callbacksOrOnMessage === 'function'
    ? { onStateUpdate: callbacksOrOnMessage }
    : callbacksOrOnMessage;

  ws.onopen = () => {
    console.log('[WS] Connected to tracker-server');
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
    console.log('[WS] Disconnected, reconnecting in 3s...');
    setTimeout(() => connectWebSocket(callbacks), 3000);
  };

  ws.onerror = (error) => {
    console.error('[WS] Error:', error);
  };

  return ws;
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
