export interface TmuxWindowInfo {
  session_id: string;
  session_name: string;
  window_id: string;
  window_name: string;
  window_index: number;
  pane_count: number;
  active: boolean;
  git_dir?: string;
}
