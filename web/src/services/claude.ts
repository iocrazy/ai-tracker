// Claude messages API

import { API_BASE, authFetch } from './auth';

export interface InteractiveOption {
  label: string;
  description: string;
}

export interface InteractiveQuestion {
  question: string;
  header: string;
  options: InteractiveOption[];
  multi_select: boolean;
}

export interface ToolInteraction {
  tool_name: string;
  questions: InteractiveQuestion[];
}

export interface ToolCallInfo {
  tool_use_id: string;
  tool_name: string;
  args_summary: string;
  args_full?: string;
}

export interface ToolResultInfo {
  tool_use_id: string;
  content: string;
  is_error: boolean;
}

export interface ClaudeMessage {
  role: string;  // "user" or "assistant"
  timestamp: string;
  text: string;
  thinking?: string;
  interaction?: ToolInteraction;
  tool_calls?: ToolCallInfo[];
  tool_results?: ToolResultInfo[];
}

// Chat message event from WebSocket (real-time JSONL push)
export interface ChatMessageEvent {
  kind: 'chat';
  session_file: string;
  messages: ClaudeMessage[];
}

export interface ClaudeMessagesResponse {
  success: boolean;
  messages: ClaudeMessage[];
  session_file: string;
}

export async function fetchClaudeMessages(
  count: number = 1,
  options?: { project?: string; session?: string; window?: string; pane?: string }
): Promise<ClaudeMessagesResponse> {
  const params = new URLSearchParams({ count: String(count) });
  if (options?.project) {
    params.append('project', options.project);
  }
  if (options?.session) {
    params.append('session', options.session);
  }
  if (options?.window) {
    params.append('window', options.window);
  }
  if (options?.pane) {
    params.append('pane', options.pane);
  }
  // Add cache-busting timestamp to prevent browser caching
  params.append('_t', String(Date.now()));
  const response = await authFetch(`${API_BASE}/claude/messages?${params}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}
