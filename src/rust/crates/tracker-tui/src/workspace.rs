//! Workspace module for Git worktree management
//!
//! Handles creating, listing, and removing Git worktrees for isolated development.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

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

/// Git worktree operations
pub struct GitWorktree {
    /// Base repository path
    repo_path: PathBuf,
    /// Directory for worktrees
    worktree_dir: PathBuf,
}

impl GitWorktree {
    /// Create a new GitWorktree manager from workspace config
    pub fn from_config(config: &WorkspaceConfig) -> Self {
        let worktree_dir = config.base_path.join(&config.worktree_dir);
        Self {
            repo_path: config.base_path.clone(),
            worktree_dir,
        }
    }

    /// Create a new GitWorktree manager
    pub fn new(repo_path: PathBuf, worktree_dir: PathBuf) -> Self {
        Self {
            repo_path,
            worktree_dir,
        }
    }

    /// List all worktrees
    pub fn list(&self) -> Result<Vec<WorktreeInfo>> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&self.repo_path)
            .output()
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

    /// Create a new worktree for a branch
    ///
    /// If the branch doesn't exist, it will be created from the current HEAD.
    pub fn create(&self, branch: &str) -> Result<PathBuf> {
        // Ensure worktree directory exists
        std::fs::create_dir_all(&self.worktree_dir)
            .with_context(|| format!("Failed to create worktree directory {:?}", self.worktree_dir))?;

        let worktree_path = self.worktree_dir.join(branch);

        // Check if worktree already exists
        if worktree_path.exists() {
            bail!("Worktree already exists at {:?}", worktree_path);
        }

        // Check if branch exists
        let branch_exists = self.branch_exists(branch)?;

        let output = if branch_exists {
            // Checkout existing branch
            Command::new("git")
                .args(["worktree", "add", worktree_path.to_str().unwrap(), branch])
                .current_dir(&self.repo_path)
                .output()
                .context("Failed to execute git worktree add")?
        } else {
            // Create new branch
            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    worktree_path.to_str().unwrap(),
                ])
                .current_dir(&self.repo_path)
                .output()
                .context("Failed to execute git worktree add -b")?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree add failed: {}", stderr);
        }

        Ok(worktree_path)
    }

    /// Remove a worktree
    pub fn remove(&self, branch: &str, force: bool) -> Result<()> {
        let worktree_path = self.worktree_dir.join(branch);

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
            .context("Failed to execute git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree remove failed: {}", stderr);
        }

        Ok(())
    }

    /// Check if a branch exists
    fn branch_exists(&self, branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{}", branch)])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to check branch existence")?;

        Ok(output.status.success())
    }

    /// Get the worktree path for a branch (if exists)
    pub fn get_worktree_path(&self, branch: &str) -> Option<PathBuf> {
        let path = self.worktree_dir.join(branch);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Prune stale worktrees
    pub fn prune(&self) -> Result<()> {
        let output = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to execute git worktree prune")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree prune failed: {}", stderr);
        }

        Ok(())
    }
}

/// Check if a path is inside a git repository
pub fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_worktree_list() {
        let output = r#"worktree /Users/test/project
branch refs/heads/main

worktree /Users/test/project/.worktrees/feature-x
branch refs/heads/feature-x

"#;
        let git = GitWorktree::new(
            PathBuf::from("/Users/test/project"),
            PathBuf::from("/Users/test/project/.worktrees"),
        );
        let worktrees = git.parse_worktree_list(output).unwrap();
        assert_eq!(worktrees.len(), 2);
        assert!(worktrees[0].is_main);
        assert!(!worktrees[1].is_main);
        assert_eq!(worktrees[1].branch, "feature-x");
    }
}
