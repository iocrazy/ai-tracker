// Workspace APIs (worktree-based windows)

import { API_BASE, authFetch } from './auth';
import { tmuxKillWindow } from './tmux';

export type LayoutType = 'simple' | 'default' | 'single_service' | 'fullstack' | 'workspace';

export interface StartWorkspaceRequest {
  git_dir: string;
  branch: string;
  base_branch?: string;  // Base branch to create new branch from
  session?: string;
  agent?: string;
  layout?: LayoutType;
  fullstack_mode?: boolean;
  port_base?: number;
  frontend_cmd?: string;
  backend_cmd?: string;
  auto_open_browser?: boolean;
}

export interface StartWorkspaceResponse {
  success: boolean;
  session_name: string;
  worktree_path: string;
  message: string;
  port?: number;
  frontend_port?: number;
  backend_port?: number;
  browser_url?: string;
}

export interface DestroyWorkspaceRequest {
  git_dir: string;
  branch: string;
  session?: string;
  force?: boolean;
  kill_ports?: boolean;
  delete_branch?: boolean;
}

export interface ResumeWorkspaceRequest {
  git_dir: string;
  branch: string;
  session?: string;
  agent?: string;
  layout?: LayoutType;
}

// Start a new workspace with worktree
export async function startWorkspace(req: StartWorkspaceRequest): Promise<StartWorkspaceResponse> {
  const response = await authFetch(`${API_BASE}/workspace/start`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return response.json();
}

// Destroy a workspace (delete worktree + tmux window)
export async function destroyWorkspace(req: DestroyWorkspaceRequest): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/workspace/destroy`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return response.json();
}

// Resume a workspace (reopen existing worktree)
export async function resumeWorkspace(req: ResumeWorkspaceRequest): Promise<StartWorkspaceResponse> {
  const response = await authFetch(`${API_BASE}/workspace/resume`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return response.json();
}

// Close a window (just close tmux window, keep worktree)
export async function closeWindow(session: string, window: string): Promise<{ success: boolean; message: string }> {
  // Just kill the tmux window without destroying worktree
  return tmuxKillWindow(session, window);
}
