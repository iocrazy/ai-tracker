// Git and config APIs

import { API_BASE, authFetch } from './auth';

export interface BranchInfo {
  name: string;
  has_worktree: boolean;
  worktree_path?: string;
}

export interface GitBranchesResponse {
  branches: string[];
  local: string[];
  remote: string[];
  branches_with_status?: BranchInfo[];
}

// Config types (from /api/config)
export interface AgentDef {
  command: string;
  color?: string;
  icon?: string;
}

export interface ConfigDefaults {
  layout: string;
  agent: string;
}

export interface ConfigResponse {
  agents: Record<string, AgentDef>;
  defaults: ConfigDefaults;
}

// Fetch server config (agents, defaults)
export async function fetchConfig(): Promise<ConfigResponse> {
  const response = await authFetch(`${API_BASE}/config`);
  if (!response.ok) {
    throw new Error(`Failed to fetch config: ${response.status}`);
  }
  return response.json();
}

// Fetch git branches for a repository
export async function fetchGitBranches(gitDir?: string): Promise<GitBranchesResponse> {
  const params = gitDir ? `?git_dir=${encodeURIComponent(gitDir)}` : '';
  const response = await authFetch(`${API_BASE}/git/branches${params}`);
  if (!response.ok) {
    throw new Error(`Failed to fetch branches: ${response.status}`);
  }
  return response.json();
}
