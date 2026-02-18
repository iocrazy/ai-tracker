# Agent Tracker — macOS Menu Bar + Floating Window (Tauri)

## Goal

Build a lightweight macOS native app (Tauri) that provides always-available monitoring of agent sessions via a Menu Bar icon + expandable panel and a pinnable Floating Window. Data sourced from existing tracker-server WebSocket at localhost:3099.

## Decisions

- **Tech stack**: Tauri v2 (Rust + React/Vite WebView) — reuse existing web code
- **Visual style**: Native macOS (vibrancy/blur, system fonts, SF-style icons)
- **Menu Bar panel**: Session list with IDLE/BUSY/OFFLINE status, expandable Claude details
- **Floating Window**: Compact status bar (session count + BUSY count + total cost)
- **Memory target**: ~30-50MB resident

## Architecture

```
┌─────────────────────────────────────┐
│  Tauri App (tauri-menubar/)         │
│  ├── src-tauri/     Rust shell      │
│  │   ├── tray icon + click handler  │
│  │   ├── menu bar window mgmt      │
│  │   └── floating window mgmt      │
│  └── src/           React UI        │
│       ├── reuse services/state.ts   │
│       ├── reuse services/dataMapper │
│       ├── reuse types.ts            │
│       └── new compact components    │
└─────────────────────────────────────┘
         │ WebSocket ws://127.0.0.1:3099/ws
         ▼
┌─────────────────────────────────────┐
│  tracker-server (existing, no changes)
└─────────────────────────────────────┘
```

## Menu Bar Icon

- Normal: monochrome template icon (hexagon or terminal prompt shape)
- BUSY sessions present: icon + numeric badge showing BUSY count
- Server offline: icon grayed out

## Menu Bar Panel (click to expand)

Native macOS popover style with vibrancy background:

```
┌─────────────────────────────┐
│  Agent Tracker    ● ONLINE  │
├─────────────────────────────┤
│  1-tracker        ● BUSY    │
│    main           ████░░ 45%│
│    $2.50 · sonnet · 01:23   │
│                             │
│  2-api            ○ IDLE    │
│    main           ░░░░░░    │
├─────────────────────────────┤
│  📌 Pin Window   ⚙ Settings│
└─────────────────────────────┘
```

- Each session collapsible, expand to show windows + Claude details (model, cost, context%, duration, current tool)
- Bottom: "Pin Window" button toggles Floating Window, Settings opens preferences
- Panel dismisses on click outside (standard macOS behavior)

## Floating Window (compact status bar)

Always-on-top, draggable, single line:

```
┌──────────────────────────────────┐
│ ⬡ 3 sessions · 1 busy · $5.20   │
└──────────────────────────────────┘
```

- Semi-transparent vibrancy background
- Click opens Menu Bar panel
- Right-click context menu: close, lock position, adjust opacity
- Remembers position across launches

## Code Reuse from web/

| Existing file | Reuse approach |
|---|---|
| `services/state.ts` | Direct reuse — WebSocket connect, reconnection, message parsing |
| `services/dataMapper.ts` | Direct reuse — mapTmuxToSessions transformation |
| `types.ts` | Direct reuse — AgentSession, AgentWindow, ClaudeStatus |
| `services/auth.ts` | Adapt — replace localStorage with Tauri secure storage |
| `WorkstationsView.tsx` | Extract STATUS_STYLES constants, rewrite as compact cards |

## Project Structure

```
ai-tracker/
├── web/                      (existing web frontend, unchanged)
├── tauri-menubar/            (new)
│   ├── src-tauri/
│   │   ├── Cargo.toml        (tauri, tauri-plugin-positioner, tauri-plugin-store)
│   │   ├── tauri.conf.json   (window config, tray, permissions)
│   │   ├── icons/            (tray icon template images)
│   │   └── src/
│   │       └── lib.rs        (tray setup, window create/show/hide, IPC commands)
│   ├── src/
│   │   ├── main.tsx           (entry point)
│   │   ├── App.tsx            (router: menu-bar panel vs floating-bar)
│   │   ├── MenuBarPanel.tsx   (session list + Claude details)
│   │   ├── FloatingBar.tsx    (compact one-line status)
│   │   ├── LoginView.tsx      (token input)
│   │   ├── hooks/
│   │   │   └── useTrackerState.ts  (WebSocket + state management)
│   │   └── shared/            (copied/symlinked from web/src)
│   │       ├── types.ts
│   │       ├── services/state.ts
│   │       └── services/dataMapper.ts
│   ├── package.json
│   ├── vite.config.ts
│   └── tailwind.config.js
```

## Tauri Rust Side (src-tauri/lib.rs)

Key responsibilities:
1. **System tray**: Create tray icon, handle click to toggle menu-bar window
2. **Menu bar window**: Positioned below tray icon via tauri-plugin-positioner, hidden/shown on tray click
3. **Floating window**: Always-on-top, decorationless, created on demand via IPC from React
4. **Secure storage**: Store auth token via tauri-plugin-store (encrypted)
5. **IPC commands**: `get_token`, `set_token`, `toggle_float`, `update_tray_badge`

## Tauri Plugins

- `tauri-plugin-positioner` — Position menu-bar window under tray icon
- `tauri-plugin-store` — Encrypted key-value store for auth token
- `tauri-plugin-autostart` — Optional: launch at login

## Data Flow

```
tracker-server (WebSocket 1s updates)
    ↓
useTrackerState() hook
    ↓ RealtimeMessage { state, tmux_windows }
    ↓
mapTmuxToSessions(tmuxWindows, tasks)
    ↓
AgentSession[] + computed stats
    ↓
┌───────────────┬──────────────────┐
│ MenuBarPanel  │ FloatingBar      │
│ (full list)   │ (summary stats)  │
└───────────────┴──────────────────┘
    ↓ IPC
Rust: update tray badge count
```

## Settings (stored in tauri-plugin-store)

- Auth token (encrypted)
- Floating window position (x, y)
- Floating window opacity (0.5-1.0)
- Launch at login (boolean)
- Server URL (default localhost:3099)

## Build & Distribution

```bash
cd tauri-menubar
npm install
npm run tauri build     # Produces .app bundle + .dmg
```

Output: `tauri-menubar/src-tauri/target/release/bundle/macos/AgentTracker.app`

No code signing needed for local use. For distribution, would need Apple Developer cert.
