//! Workspace module for Git worktree management (async version)
//!
//! Handles creating, listing, and removing Git worktrees for isolated development.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

use crate::config::WorkspaceConfig;

/// Information about an active worktree
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Path to the worktree
    pub path: PathBuf,
    /// Branch name
    pub branch: String,
    /// Whether this is the main worktree
    pub is_main: bool,
}

/// Git worktree operations (async)
pub struct GitWorktree {
    /// Base repository path
    repo_path: PathBuf,
    /// Directory for worktrees
    worktree_dir: PathBuf,
}

impl GitWorktree {
    /// Create a new GitWorktree manager from a git repository path
    ///
    /// If the given path is a worktree, automatically resolves to the main repository.
    /// Worktrees will be created in `.worktrees/` under the main repo path.
    pub fn new(repo_path: &std::path::Path) -> Self {
        // Resolve to main repository if this is a worktree
        let main_repo = Self::resolve_main_repo(repo_path);
        let worktree_dir = main_repo.join(".worktrees");
        Self {
            repo_path: main_repo,
            worktree_dir,
        }
    }

    /// Resolve a path to its main repository (if it's a worktree)
    ///
    /// Git worktrees have a `.git` file (not directory) that points to the main repo.
    /// This function detects this and returns the main repository path.
    fn resolve_main_repo(path: &std::path::Path) -> PathBuf {
        let git_path = path.join(".git");

        // Check if .git is a file (worktree) or directory (main repo)
        if git_path.is_file() {
            // Read the .git file to find the main repo
            if let Ok(content) = std::fs::read_to_string(&git_path) {
                // Format: "gitdir: /path/to/main/.git/worktrees/name"
                if let Some(gitdir) = content.strip_prefix("gitdir: ") {
                    let gitdir = gitdir.trim();
                    // The gitdir points to .git/worktrees/name, we need to go up to find .git
                    let gitdir_path = std::path::Path::new(gitdir);
                    // Go up from .git/worktrees/name to .git, then to repo root
                    if let Some(git_dir) = gitdir_path.parent().and_then(|p| p.parent()) {
                        if let Some(main_repo) = git_dir.parent() {
                            return main_repo.to_path_buf();
                        }
                    }
                }
            }
        }

        // Not a worktree or couldn't resolve, use as-is
        path.to_path_buf()
    }

    /// Create a new GitWorktree manager from workspace config
    pub fn from_config(config: &WorkspaceConfig) -> Self {
        let worktree_dir = config.base_path.join(&config.worktree_dir);
        Self {
            repo_path: config.base_path.clone(),
            worktree_dir,
        }
    }

    /// List all worktrees
    pub async fn list(&self) -> Result<Vec<WorktreeInfo>> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("Failed to execute git worktree list")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree list failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_worktree_list(&stdout)
    }

    /// Parse porcelain output of `git worktree list`
    fn parse_worktree_list(&self, output: &str) -> Result<Vec<WorktreeInfo>> {
        let mut worktrees = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;
        let mut is_bare = false;

        for line in output.lines() {
            if line.starts_with("worktree ") {
                // Save previous worktree if any
                if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                    if !is_bare {
                        let is_main = !path.starts_with(&self.worktree_dir);
                        worktrees.push(WorktreeInfo {
                            path,
                            branch,
                            is_main,
                        });
                    }
                }
                current_path = Some(PathBuf::from(line.strip_prefix("worktree ").unwrap()));
                current_branch = None;
                is_bare = false;
            } else if line.starts_with("branch ") {
                let branch = line.strip_prefix("branch refs/heads/").unwrap_or(
                    line.strip_prefix("branch ").unwrap_or(line),
                );
                current_branch = Some(branch.to_string());
            } else if line == "bare" {
                is_bare = true;
            } else if line == "detached" {
                current_branch = Some("(detached)".to_string());
            }
        }

        // Don't forget the last worktree
        if let (Some(path), Some(branch)) = (current_path, current_branch) {
            if !is_bare {
                let is_main = !path.starts_with(&self.worktree_dir);
                worktrees.push(WorktreeInfo {
                    path,
                    branch,
                    is_main,
                });
            }
        }

        Ok(worktrees)
    }

    /// Sanitize branch name for use as directory name
    ///
    /// Replaces `:` and `/` with `-` for filesystem compatibility.
    fn sanitize_for_dirname(branch: &str) -> String {
        branch.replace([':', '/'], "-")
    }

    /// Convert branch name for git (`:` -> `/`)
    ///
    /// Git standard: `fix:auth-bug` -> `fix/auth-bug`
    fn sanitize_for_git(branch: &str) -> String {
        branch.replace(':', "/")
    }

    /// Create a new worktree for a branch
    ///
    /// If the branch doesn't exist, it will be created from the current HEAD.
    /// Branch names like "fix:bug" become:
    /// - Git branch: `fix/bug` (git standard)
    /// - Directory: `.worktrees/fix-bug` (filesystem safe)
    pub async fn create(&self, branch: &str) -> Result<PathBuf> {
        // Ensure worktree directory exists
        tokio::fs::create_dir_all(&self.worktree_dir)
            .await
            .with_context(|| format!("Failed to create worktree directory {:?}", self.worktree_dir))?;

        // Sanitize branch name for directory (: and / -> -)
        let dir_name = Self::sanitize_for_dirname(branch);
        let worktree_path = self.worktree_dir.join(&dir_name);

        // Convert branch name for git (: -> /)
        let git_branch = Self::sanitize_for_git(branch);

        // Check if worktree already exists
        if worktree_path.exists() {
            bail!("Worktree already exists at {:?}", worktree_path);
        }

        // Check if branch exists
        let branch_exists = self.branch_exists(&git_branch).await?;

        let output = if branch_exists {
            // Checkout existing branch
            Command::new("git")
                .args(["worktree", "add", worktree_path.to_str().unwrap(), &git_branch])
                .current_dir(&self.repo_path)
                .output()
                .await
                .context("Failed to execute git worktree add")?
        } else {
            // Create new branch
            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "-b",
                    &git_branch,
                    worktree_path.to_str().unwrap(),
                ])
                .current_dir(&self.repo_path)
                .output()
                .await
                .context("Failed to execute git worktree add -b")?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree add failed: {}", stderr);
        }

        Ok(worktree_path)
    }

    /// Remove a worktree
    pub async fn remove(&self, branch: &str, force: bool) -> Result<()> {
        let dir_name = Self::sanitize_for_dirname(branch);
        let worktree_path = self.worktree_dir.join(&dir_name);

        if !worktree_path.exists() {
            bail!("Worktree not found at {:?}", worktree_path);
        }

        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(worktree_path.to_str().unwrap());

        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("Failed to execute git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree remove failed: {}", stderr);
        }

        Ok(())
    }

    /// Check if a branch exists
    async fn branch_exists(&self, branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{}", branch)])
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("Failed to check branch existence")?;

        Ok(output.status.success())
    }

    /// Get the worktree path for a branch (if exists)
    pub fn get_worktree_path(&self, branch: &str) -> Option<PathBuf> {
        let dir_name = Self::sanitize_for_dirname(branch);
        let path = self.worktree_dir.join(&dir_name);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Create a feature directory with worktree inside
    ///
    /// Structure:
    /// ```
    /// agents/features/{branch-name}/
    /// ├── repo/           # git worktree
    /// ├── feature.json    # metadata (created separately)
    /// └── .agent-info     # status info (created separately)
    /// ```
    ///
    /// If `base_branch` is provided, the new branch will be created from it.
    /// Otherwise, it will be created from main/master.
    pub async fn create_feature_dir(&self, branch: &str, base_branch: Option<&str>) -> Result<PathBuf> {
        let dir_name = Self::sanitize_for_dirname(branch);
        let feature_dir = self.repo_path.join("agents").join("features").join(&dir_name);
        let worktree_path = feature_dir.join("repo");

        // Create feature directory structure
        tokio::fs::create_dir_all(&feature_dir)
            .await
            .with_context(|| format!("Failed to create feature directory {:?}", feature_dir))?;

        // Convert branch name for git (: -> /)
        let git_branch = Self::sanitize_for_git(branch);

        // Check if worktree already exists
        if worktree_path.exists() {
            bail!("Feature worktree already exists at {:?}", worktree_path);
        }

        // Check if branch exists
        let branch_exists = self.branch_exists(&git_branch).await?;

        let output = if branch_exists {
            // Checkout existing branch
            Command::new("git")
                .args(["worktree", "add", worktree_path.to_str().unwrap(), &git_branch])
                .current_dir(&self.repo_path)
                .output()
                .await
                .context("Failed to execute git worktree add")?
        } else {
            // Create new branch from base_branch or main/master
            let start_point = if let Some(base) = base_branch {
                Self::sanitize_for_git(base)
            } else {
                self.detect_main_branch().await?
            };
            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "-b",
                    &git_branch,
                    worktree_path.to_str().unwrap(),
                    &start_point,
                ])
                .current_dir(&self.repo_path)
                .output()
                .await
                .context("Failed to execute git worktree add -b")?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree add failed: {}", stderr);
        }

        Ok(worktree_path)
    }

    /// Clean up a feature directory (removes worktree and feature dir)
    pub async fn cleanup_feature_dir(&self, branch: &str, force: bool) -> Result<()> {
        let dir_name = Self::sanitize_for_dirname(branch);
        let feature_dir = self.repo_path.join("agents").join("features").join(&dir_name);
        let worktree_path = feature_dir.join("repo");

        // Remove worktree if exists
        if worktree_path.exists() {
            let mut args = vec!["worktree", "remove"];
            if force {
                args.push("--force");
            }
            args.push(worktree_path.to_str().unwrap());

            let output = Command::new("git")
                .args(&args)
                .current_dir(&self.repo_path)
                .output()
                .await
                .context("Failed to execute git worktree remove")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // If git worktree remove fails, try manual removal
                if force {
                    tokio::fs::remove_dir_all(&worktree_path)
                        .await
                        .with_context(|| format!("Failed to remove worktree directory {:?}", worktree_path))?;
                } else {
                    bail!("git worktree remove failed: {}", stderr);
                }
            }
        }

        // Remove feature directory if empty or force
        if feature_dir.exists() {
            if force {
                tokio::fs::remove_dir_all(&feature_dir)
                    .await
                    .with_context(|| format!("Failed to remove feature directory {:?}", feature_dir))?;
            } else {
                // Try to remove if empty
                let _ = tokio::fs::remove_dir(&feature_dir).await;
            }
        }

        Ok(())
    }

    /// Delete a git branch
    pub async fn delete_branch(&self, branch: &str, force: bool) -> Result<()> {
        let git_branch = Self::sanitize_for_git(branch);

        let flag = if force { "-D" } else { "-d" };
        let output = Command::new("git")
            .args(["branch", flag, &git_branch])
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("Failed to execute git branch -d")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git branch delete failed: {}", stderr);
        }

        Ok(())
    }

    /// Detect the main branch name (main or master)
    async fn detect_main_branch(&self) -> Result<String> {
        // Check for 'main' branch
        if self.branch_exists("main").await? {
            return Ok("main".to_string());
        }
        // Check for 'master' branch
        if self.branch_exists("master").await? {
            return Ok("master".to_string());
        }
        // Default to 'main'
        Ok("main".to_string())
    }

    /// Get the feature directory path for a branch
    pub fn get_feature_dir(&self, branch: &str) -> PathBuf {
        let dir_name = Self::sanitize_for_dirname(branch);
        self.repo_path.join("agents").join("features").join(&dir_name)
    }

    /// List all local branches
    pub async fn list_branches(&self) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("Failed to execute git branch")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git branch failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let branches: Vec<String> = stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(branches)
    }

    /// List remote branches (origin/*)
    pub async fn list_remote_branches(&self) -> Result<Vec<String>> {
        // First fetch to update remote refs
        let _ = Command::new("git")
            .args(["fetch", "--prune"])
            .current_dir(&self.repo_path)
            .output()
            .await;

        let output = Command::new("git")
            .args(["branch", "-r", "--format=%(refname:short)"])
            .current_dir(&self.repo_path)
            .output()
            .await
            .context("Failed to execute git branch -r")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git branch -r failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let branches: Vec<String> = stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.contains("HEAD") && !s.contains("->"))
            // Remove "origin/" prefix for cleaner display
            .map(|s| s.strip_prefix("origin/").unwrap_or(&s).to_string())
            // Filter out "origin" itself (from origin/HEAD -> origin/master)
            .filter(|s| s != "origin" && !s.is_empty())
            .collect();

        Ok(branches)
    }

    /// Find a worktree path for a given branch (static method)
    ///
    /// This method searches all worktrees in a repository to find one matching the branch name.
    /// Returns the path to the worktree if found.
    pub async fn find_worktree(repo_path: &std::path::Path, branch: &str) -> Result<Option<PathBuf>> {
        let git_worktree = GitWorktree::new(repo_path);
        let worktrees = git_worktree.list().await?;

        // Sanitize branch name for comparison (handle : -> /)
        let git_branch = Self::sanitize_for_git(branch);

        for wt in worktrees {
            if wt.branch == git_branch || wt.branch == branch {
                return Ok(Some(wt.path));
            }
        }

        // Also check by directory name
        let dir_name = Self::sanitize_for_dirname(branch);
        let worktree_path = git_worktree.worktree_dir.join(&dir_name);
        if worktree_path.exists() {
            return Ok(Some(worktree_path));
        }

        Ok(None)
    }

    /// List all branches (local + remote, deduplicated)
    pub async fn list_all_branches(&self) -> Result<Vec<String>> {
        let local = self.list_branches().await.unwrap_or_default();
        let remote = self.list_remote_branches().await.unwrap_or_default();

        let mut all: Vec<String> = local;
        for branch in remote {
            if !all.contains(&branch) {
                all.push(branch);
            }
        }

        // Sort branches, putting main/master first
        all.sort_by(|a, b| {
            let a_is_main = a == "main" || a == "master";
            let b_is_main = b == "main" || b == "master";
            match (a_is_main, b_is_main) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });

        Ok(all)
    }
}
