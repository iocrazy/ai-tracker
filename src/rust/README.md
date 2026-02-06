# Agent Tracker (Rust)

Rust rewrite of the Agent Tracker system.

## Project Structure

```
rust/
├── Cargo.toml              # Workspace configuration
└── crates/
    ├── tracker-core/       # Core types and IPC protocol
    ├── tracker-server/     # Server daemon (Unix socket)
    ├── tracker-tui/        # Terminal UI client
    └── tracker-web/        # Web API server
```

## Building

```bash
# Build all crates
cargo build

# Build release
cargo build --release

# Run specific binary
cargo run -p tracker-server
cargo run -p tracker-tui
cargo run -p tracker-web
```

## Development Status

### tracker-core ✅
- [x] IPC Envelope type
- [x] Task/Note/Goal types
- [x] Command constants

### tracker-server ✅
- [x] Basic Unix socket server
- [x] Task CRUD (start/finish/pause/delete)
- [x] State broadcast
- [x] SQLite persistence
- [x] History archiving
- [x] Note management (add/edit/delete/archive/toggle)
- [x] Goal management (add/delete/toggle)

### tracker-tui ✅
- [x] Basic ratatui setup
- [x] Task list display
- [x] Keyboard navigation (j/k)
- [x] Tab switching (Tasks/Notes/Goals)
- [x] Note list view with scope badge
- [x] Goal list view
- [x] Actions (delete/toggle/archive)
- [x] Search filter (/ key)
- [x] Session filter (s key)

### tracker-web ✅
- [x] Basic axum setup
- [x] Health endpoint
- [x] CORS support
- [x] Connect to tracker-server
- [x] REST API (tasks/notes/goals)
- [x] Command API (send commands)
- [x] WebSocket real-time updates
- [ ] tmux session/window listing
- [ ] tmux send-keys
- [ ] Authentication

## Testing

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run -p tracker-server
```

## Migration Guide

This Rust version is designed to be compatible with the Go version:
- Same Unix socket path: `/tmp/agent-tracker.sock`
- Same JSON protocol (Envelope)
- Can run side-by-side during development

To switch from Go to Rust:
1. Stop Go server: `brew services stop tracker-server`
2. Run Rust server: `cargo run -p tracker-server --release`
