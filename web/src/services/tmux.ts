// Tmux operations API

import { API_BASE, authFetch, getAuthToken } from './auth';

// Tmux window info (from /api/tmux/windows)
export interface TmuxWindowInfo {
  session_id: string;
  session_name: string;
  window_id: string;
  window_name: string;
  window_index: number;
  pane_count: number;
  active: boolean;
  git_dir?: string;  // Git directory for the session
  working_dir?: string;  // Active pane's current path (volatile)
  agent_dir?: string;    // Stable worktree/working path recorded at creation (@agent_dir)
}

// Fetch all tmux windows with full details
export async function fetchTmuxWindows(): Promise<TmuxWindowInfo[]> {
  const response = await authFetch(`${API_BASE}/tmux/windows`);
  if (!response.ok) {
    throw new Error(`Failed to fetch tmux windows: ${response.status}`);
  }
  const data = await response.json();
  return data.windows || [];
}

// Execute tmux send-keys command
// suffixKey: key to send after the text (e.g., "Enter", "C-m", "C-s", or empty for none)
export async function tmuxSendKeys(
  session: string,
  window: string,
  pane: string,
  keys: string,
  suffixKey: string = 'Enter'
): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/send-keys`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window, pane, keys, suffix_key: suffixKey }),
  });
  return response.json();
}

// Send image(s) to a tmux pane (saves to temp file, sends path via send-keys)
export async function sendImage(
  session: string,
  windowId: string,
  pane: string,
  imageBase64: string,
  message?: string
): Promise<{ success: boolean; message: string; image_path?: string }> {
  const response = await authFetch(`${API_BASE}/tmux/send-image`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      session,
      window_id: windowId,
      pane,
      image_base64: imageBase64,
      message,
    }),
  });
  return response.json();
}

// Send multiple images to a tmux pane
export async function sendImages(
  session: string,
  windowId: string,
  pane: string,
  imagesBase64: string[],
  message?: string,
  onProgress?: (percent: number) => void
): Promise<{ success: boolean; message: string; image_paths?: string[] }> {
  const body = JSON.stringify({
    session,
    window_id: windowId,
    pane,
    images: imagesBase64,
    message,
  });

  // Use XHR to get upload progress events
  const token = getAuthToken();
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', `${API_BASE}/tmux/send-image`);
    xhr.setRequestHeader('Content-Type', 'application/json');
    if (token) xhr.setRequestHeader('Authorization', `Bearer ${token}`);
    if (onProgress && xhr.upload) {
      xhr.upload.onprogress = (e) => {
        if (e.lengthComputable) {
          onProgress(Math.round((e.loaded / e.total) * 100));
        }
      };
    }
    xhr.onload = () => {
      try { resolve(JSON.parse(xhr.responseText)); }
      catch (err) { reject(err); }
    };
    xhr.onerror = () => reject(new Error('Network error'));
    xhr.onabort = () => reject(new Error('Upload aborted'));
    xhr.send(body);
  });
}

/**
 * Send a sequence of raw tmux key names (e.g., ["Down", "Space", "Enter"]).
 * Each key is sent as a raw tmux key (not literal text) via the send-keys API.
 * Used for multi-select TUI navigation where arrow keys and Space are needed.
 * Includes inter-key delay to prevent key coalescing in TUI frameworks.
 */
export async function tmuxSendRawKeys(
  session: string,
  window: string,
  pane: string,
  keys: string[],
  delayMs: number = 50,
): Promise<void> {
  for (const key of keys) {
    // Extra delay before Enter/Space to let TUI process navigation first
    const isAction = key === 'Enter' || key === 'Space';
    if (isAction && delayMs > 0) {
      await new Promise(r => setTimeout(r, 300));
    }
    const result = await tmuxSendKeys(session, window, pane, '', key);
    if (!result.success) {
      throw new Error(`Failed to send key "${key}": ${result.message}`);
    }
    if (delayMs > 0) {
      await new Promise(r => setTimeout(r, delayMs));
    }
  }
}

// Execute arbitrary tmux command (parse and route to appropriate API)
export async function executeTmuxCommand(command: string): Promise<{ success: boolean; message: string }> {
  // Parse tmux send-keys command with quoted keys
  // Format: tmux send-keys -t session:window "keys" [suffix_key]
  // Or: tmux send-keys -t session:window.pane "keys" [suffix_key]
  // suffix_key can be: C-m, C-s, Enter, etc.
  const sendKeysMatch = command.match(/^tmux\s+send-keys\s+-t\s+([^:]+):([^.\s"]+)(?:\.([^\s"]+))?\s+"([^"]+)"\s*(C-[a-z]|Enter)?$/);
  if (sendKeysMatch) {
    const [, session, window, pane = '', keys, suffixKey = ''] = sendKeysMatch;
    return tmuxSendKeys(session, window, pane, keys, suffixKey);
  }

  // Parse tmux send-keys without quotes
  const sendKeysMatch2 = command.match(/^tmux\s+send-keys\s+-t\s+([^:]+):([^.\s]+)(?:\.([^\s]+))?\s+([^\s].+?)\s*(C-[a-z]|Enter)?$/);
  if (sendKeysMatch2) {
    const [, session, window, pane = '', keys, suffixKey = ''] = sendKeysMatch2;
    return tmuxSendKeys(session, window, pane, keys, suffixKey);
  }

  return { success: false, message: `Unknown command format: ${command}` };
}

// Kill a tmux session
export async function tmuxKillSession(session: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/kill-session`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Kill a tmux window
export async function tmuxKillWindow(session: string, window: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/kill-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Create a new tmux window
export async function tmuxNewWindow(session: string, name: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/new-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, name }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Closed window info
export interface ClosedWindow {
  id: number;
  session_name: string;
  window_name: string;
  working_dir: string;
  git_branch: string;
  pane_count: number;
  closed_at: string | null;
}

// Get closed windows for a session (for resume without worktree)
export async function fetchClosedWindows(sessionName: string): Promise<ClosedWindow[]> {
  const response = await authFetch(`${API_BASE}/tmux/closed-windows/${encodeURIComponent(sessionName)}`);
  if (!response.ok) {
    return [];
  }
  return response.json();
}

// Delete a closed window record
export async function deleteClosedWindow(id: number): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/closed-windows`, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Resume a closed window with optional layout
export async function resumeClosedWindow(
  session: string,
  windowName: string,
  workingDir: string,
  layout?: 'simple' | 'default' | 'workspace',
  closedWindowId?: number
): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/resume-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      session,
      window_name: windowName,
      working_dir: workingDir,
      layout: layout || 'simple',
      closed_window_id: closedWindowId,
    }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Swap two windows within a session (for drag-and-drop reordering)
export async function tmuxSwapWindow(
  session: string,
  sourceIndex: number,
  targetIndex: number
): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/swap-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, source_index: sourceIndex, target_index: targetIndex }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Select (switch to) a tmux window
// windowId is optional - use it for precise targeting when windows have duplicate names
export async function tmuxSelectWindow(session: string, window: string, windowId?: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/select-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window, window_id: windowId }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Rename a tmux window
export async function tmuxRenameWindow(session: string, window: string, name: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/rename-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window, name }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Reset window layout to default 3-pane (yazi + lazygit + agent)
export async function tmuxResetLayout(session: string, window: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/reset-layout`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Rename a tmux session
export async function tmuxRenameSession(session: string, name: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/rename-session`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, name }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Claude status API
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
  pending_menu?: {
    header: string;
    question?: string;
    options: { index: number; label: string; description: string; selected: boolean; checked: boolean }[];
    preview?: string;
    multi_select: boolean;
  };
}

export interface ClaudeStatusResponse {
  success: boolean;
  status: ClaudeStatus;
}

export async function fetchClaudeStatus(
  session: string,
  window: string
): Promise<ClaudeStatusResponse> {
  const params = new URLSearchParams({ session, window });
  const response = await authFetch(`${API_BASE}/tmux/claude-status?${params}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Tmux capture API
export interface TmuxCaptureResponse {
  content: string;
  cursor_x: number;
  cursor_y: number;
}

export async function fetchTmuxCapture(
  session: string,
  window: string,
  pane: string = '',
  lines?: number
): Promise<TmuxCaptureResponse> {
  const params = new URLSearchParams({ session, window, pane });
  if (lines) {
    params.append('lines', String(lines));
  }
  const response = await authFetch(`${API_BASE}/tmux/capture?${params}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}
