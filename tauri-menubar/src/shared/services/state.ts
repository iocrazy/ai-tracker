// State management and WebSocket — adapted from web/src/services/state.ts

import { WS_BASE, getAuthToken } from './auth';
import type { TmuxWindowInfo } from './tmux';

// Backend types
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

export interface BackendState {
  kind: string;
  tasks: BackendTask[];
  archived_tasks: BackendTask[];
  notes: unknown[];
  archived: unknown[];
  goals: unknown[];
  history: unknown[];
  message: string;
  changed?: string[];
}

export interface RealtimeMessage {
  state: BackendState;
  tmux_windows: TmuxWindowInfo[];
}

export interface StreamChunk {
  pane_id: string;
  target: string;
  raw: string;
  text: string;
  timestamp: string;
}

export type ConnectionStatus = 'connected' | 'reconnecting' | 'offline';

interface WebSocketCallbacks {
  onStateUpdate: (msg: RealtimeMessage) => void;
  onStreamChunk?: (chunk: StreamChunk) => void;
  onConnectionChange?: (status: ConnectionStatus, retryCount?: number) => void;
}

// Reconnection state
let _retryCount = 0;
let _reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let _lastCallbacks: WebSocketCallbacks | null = null;
let _currentWs: WebSocket | null = null;
const BASE_DELAY = 1000;
const MAX_DELAY = 30000;

export function connectWebSocket(callbacks: WebSocketCallbacks): WebSocket {
  if (_reconnectTimer) {
    clearTimeout(_reconnectTimer);
    _reconnectTimer = null;
  }
  _lastCallbacks = callbacks;

  const token = getAuthToken();
  const wsUrl = token ? `${WS_BASE}?token=${encodeURIComponent(token)}` : WS_BASE;
  const ws = new WebSocket(wsUrl);
  _currentWs = ws;

  ws.onopen = () => {
    console.log('[WS] Connected to tracker-server');
    _retryCount = 0;
    callbacks.onConnectionChange?.('connected');
  };

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);

      if (data.kind === 'stream' && callbacks.onStreamChunk) {
        callbacks.onStreamChunk(data.chunk as StreamChunk);
        return;
      }

      if (data.state && data.tmux_windows) {
        callbacks.onStateUpdate(data as RealtimeMessage);
      } else if (data.kind === 'state') {
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

/** Force an immediate reconnection attempt, cancelling any pending backoff timer. */
export function reconnectNow(): WebSocket | null {
  if (!_lastCallbacks) return null;
  // Close existing socket if still open
  if (_currentWs && _currentWs.readyState <= WebSocket.OPEN) {
    _currentWs.close();
  }
  _retryCount = 0;
  return connectWebSocket(_lastCallbacks);
}
