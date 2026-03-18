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

// Reconnection state (module-level so it persists across calls)
let _retryCount = 0;
let _reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let _lastCallbacks: WebSocketCallbacks | null = null;
let _currentWs: WebSocket | null = null;
let _intentionalClose = false;  // Suppress auto-reconnect on explicit disconnect
let _lastConnectTime = 0;  // Debounce rapid-fire connect calls
const BASE_DELAY = 1000;
const MAX_DELAY = 30000;
const CONNECT_DEBOUNCE_MS = 3000;  // Min 3s between connect attempts

export function connectWebSocket(callbacks: WebSocketCallbacks): WebSocket {
  // Debounce: if we just connected within 3s, return existing WS
  const now = Date.now();
  if (_currentWs && (now - _lastConnectTime) < CONNECT_DEBOUNCE_MS
      && _currentWs.readyState <= WebSocket.OPEN) {
    console.log('[WS] Debounced — already connected/connecting within 3s');
    return _currentWs;
  }
  _lastConnectTime = now;

  // Clear any pending reconnect timer
  if (_reconnectTimer) {
    clearTimeout(_reconnectTimer);
    _reconnectTimer = null;
  }

  // Close any existing WS to prevent duplicate connections
  if (_currentWs && _currentWs.readyState <= WebSocket.OPEN) {
    _intentionalClose = true;
    _currentWs.close();
  }

  _lastCallbacks = callbacks;

  const token = getAuthToken();
  if (!token) {
    // No token yet — don't connect, let caller retry later
    console.log('[WS] No auth token available, skipping connection');
    _lastConnectTime = 0; // Reset debounce so retry works immediately
    return null as unknown as WebSocket;
  }
  const wsUrl = `${WS_BASE}?token=${encodeURIComponent(token)}`;
  const ws = new WebSocket(wsUrl);
  _currentWs = ws;

  let pingInterval: ReturnType<typeof setInterval> | null = null;

  ws.onopen = () => {
    console.log('[WS] Connected to tracker-server');
    _retryCount = 0;
    callbacks.onConnectionChange?.('connected');
    // Client-side heartbeat: send ping every 25s to keep connection alive
    // (critical for Tauri WKWebView which may suspend JS when panel is hidden)
    pingInterval = setInterval(() => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send('ping');
      }
    }, 25000);
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
    if (pingInterval) { clearInterval(pingInterval); pingInterval = null; }

    // Skip auto-reconnect if this was an intentional close (cleanup or replacement)
    if (_intentionalClose) {
      _intentionalClose = false;
      return;
    }

    // Only auto-reconnect if this is still the current WS (avoid ghost reconnects)
    if (_currentWs !== ws) return;

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
  // Use intentionalClose to prevent onclose from also triggering reconnect
  if (_currentWs && _currentWs.readyState <= WebSocket.OPEN) {
    _intentionalClose = true;
    _currentWs.close();
  }
  _retryCount = 0;
  _lastConnectTime = 0;  // Clear debounce so reconnectNow always works
  return connectWebSocket(_lastCallbacks);
}

/** Disconnect WebSocket without auto-reconnect */
export function disconnectWebSocket(): void {
  if (_reconnectTimer) {
    clearTimeout(_reconnectTimer);
    _reconnectTimer = null;
  }
  if (_currentWs) {
    _intentionalClose = true;
    _currentWs.close();
    _currentWs = null;
  }
}

/** Get current WebSocket instance (for readyState checks) */
export function getCurrentWs(): WebSocket | null {
  return _currentWs;
}
