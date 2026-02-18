# Tauri Menu Bar + Floating Window — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Tauri v2 macOS app that shows agent session status in a Menu Bar panel and optional floating status bar, reusing existing React/TypeScript code from web/.

**Architecture:** Tauri v2 Rust shell manages tray icon + two WebView windows (menu-bar panel, floating bar). React frontend connects to tracker-server WebSocket at localhost:3099, reuses existing services/types from web/. Two windows share state via the same WebSocket connection but render different compact views.

**Tech Stack:** Tauri v2, React 19, Vite, Tailwind CSS, TypeScript, tauri-plugin-positioner, tauri-plugin-store

---

### Task 1: Scaffold Tauri Project

**Files:**
- Create: `tauri-menubar/package.json`
- Create: `tauri-menubar/src-tauri/Cargo.toml`
- Create: `tauri-menubar/src-tauri/tauri.conf.json`
- Create: `tauri-menubar/src-tauri/capabilities/default.json`
- Create: `tauri-menubar/vite.config.ts`
- Create: `tauri-menubar/tsconfig.json`
- Create: `tauri-menubar/tailwind.config.js`
- Create: `tauri-menubar/postcss.config.js`
- Create: `tauri-menubar/index.html`
- Create: `tauri-menubar/src/main.tsx`

**Step 1: Create project directory and package.json**

```bash
mkdir -p tauri-menubar/src
```

Write `tauri-menubar/package.json`:
```json
{
  "name": "agent-tracker-menubar",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-positioner": "^2",
    "@tauri-apps/plugin-store": "^2",
    "lucide-react": "^0.468.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.4",
    "autoprefixer": "^10.4.20",
    "postcss": "^8.4.49",
    "tailwindcss": "^3.4.17",
    "typescript": "~5.6.0",
    "vite": "^5.4.0"
  }
}
```

**Step 2: Create Vite config**

Write `tauri-menubar/vite.config.ts`:
```typescript
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
});
```

**Step 3: Create Tailwind + PostCSS config**

Write `tauri-menubar/tailwind.config.js`:
```javascript
/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: { extend: {} },
  plugins: [],
};
```

Write `tauri-menubar/postcss.config.js`:
```javascript
export default {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
};
```

**Step 4: Create TypeScript config**

Write `tauri-menubar/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2021",
    "useDefineForClassFields": true,
    "lib": ["ES2021", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src"]
}
```

**Step 5: Create HTML entry point**

Write `tauri-menubar/index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Agent Tracker</title>
    <style>
      html, body { margin: 0; padding: 0; overflow: hidden; background: transparent; }
    </style>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

**Step 6: Create React entry point**

Write `tauri-menubar/src/main.tsx`:
```tsx
import React from 'react';
import ReactDOM from 'react-dom/client';
import './index.css';

function App() {
  return <div className="p-4 text-white bg-black">Agent Tracker Loading...</div>;
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

Write `tauri-menubar/src/index.css`:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;
```

**Step 7: Create Tauri Rust project**

```bash
mkdir -p tauri-menubar/src-tauri/src
mkdir -p tauri-menubar/src-tauri/icons
mkdir -p tauri-menubar/src-tauri/capabilities
```

Write `tauri-menubar/src-tauri/Cargo.toml`:
```toml
[package]
name = "agent-tracker-menubar"
version = "0.1.0"
edition = "2021"

[lib]
name = "agent_tracker_menubar_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-positioner = { version = "2", features = ["tray-icon"] }
tauri-plugin-store = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Write `tauri-menubar/src-tauri/build.rs`:
```rust
fn main() {
    tauri_build::build()
}
```

Write `tauri-menubar/src-tauri/tauri.conf.json`:
```json
{
  "productName": "Agent Tracker",
  "version": "0.1.0",
  "identifier": "com.agent-tracker.menubar",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [],
    "security": {
      "csp": null
    },
    "trayIcon": {
      "iconPath": "icons/icon.png",
      "iconAsTemplate": true
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

Write `tauri-menubar/src-tauri/capabilities/default.json`:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capabilities",
  "windows": ["panel", "float"],
  "permissions": [
    "core:default",
    "core:window:default",
    "core:window:allow-show",
    "core:window:allow-hide",
    "core:window:allow-set-focus",
    "core:window:allow-is-visible",
    "core:window:allow-close",
    "core:window:allow-set-position",
    "core:window:allow-set-size",
    "core:window:allow-set-always-on-top",
    "positioner:default",
    "store:default"
  ]
}
```

Write minimal `tauri-menubar/src-tauri/src/lib.rs`:
```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Write `tauri-menubar/src-tauri/src/main.rs`:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    agent_tracker_menubar_lib::run();
}
```

**Step 8: Generate placeholder tray icon**

Create a simple 22x22 PNG tray icon (macOS template icon). For now, use a placeholder:

```bash
cd tauri-menubar/src-tauri/icons
# Generate placeholder icons using sips or a script
# For development, copy from Tauri default icons
```

Run `npm create tauri-app@latest -- --template react-ts` in a temp dir to grab default icons, or create minimal ones.

**Step 9: Install dependencies and verify build**

```bash
cd tauri-menubar
npm install
npm run tauri build -- --debug 2>&1 | tail -20
```

Expected: Tauri app builds successfully (may take a few minutes on first build).

**Step 10: Commit**

```bash
git add tauri-menubar/
git commit -m "feat: scaffold Tauri v2 menubar project with React/Vite/Tailwind"
```

---

### Task 2: Tray Icon + Menu Bar Window (Rust)

**Files:**
- Modify: `tauri-menubar/src-tauri/src/lib.rs`

**Step 1: Implement tray icon with click-to-toggle panel**

Write `tauri-menubar/src-tauri/src/lib.rs`:
```rust
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

#[tauri::command]
fn show_float(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("float") {
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
    } else {
        let win = WebviewWindowBuilder::new(&app, "float", WebviewUrl::App("index.html".into()))
            .title("Agent Tracker")
            .inner_size(320.0, 36.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .build()
            .map_err(|e| e.to_string())?;
        // Position will be set by frontend via positioner plugin or manual placement
        let _ = win.set_focus();
    }
    Ok(())
}

#[tauri::command]
fn hide_float(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("float") {
        win.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn toggle_panel(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("panel") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            // Move to tray position before showing
            use tauri_plugin_positioner::{Position, WindowExt};
            let _ = win.move_window(Position::TrayCenter);
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn create_panel(app: &tauri::AppHandle) {
    let win = WebviewWindowBuilder::new(app, "panel", WebviewUrl::App("index.html".into()))
        .title("Agent Tracker")
        .inner_size(380.0, 480.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .resizable(false)
        .build();

    if let Ok(win) = win {
        // Hide panel when it loses focus
        let win_clone = win.clone();
        win.on_window_event(move |event| {
            if let tauri::WindowEvent::Focused(false) = event {
                let _ = win_clone.hide();
            }
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![show_float, hide_float])
        .setup(|app| {
            // Create the panel window (hidden initially)
            create_panel(app.handle());

            // Create tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .on_tray_icon_event(|tray, event| {
                    // Required for positioner tray-relative positioning
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_panel(tray.app_handle());
                    }
                })
                .build(app)?;

            // Hide from dock on macOS (menu bar only app)
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**Step 2: Verify it compiles**

```bash
cd tauri-menubar && npm run tauri build -- --debug 2>&1 | tail -20
```

Expected: Build succeeds. The app should show a tray icon; clicking it toggles a blank panel.

**Step 3: Commit**

```bash
git add tauri-menubar/src-tauri/src/lib.rs
git commit -m "feat: tray icon + menu bar panel + floating window management"
```

---

### Task 3: Copy Shared Code from web/

**Files:**
- Create: `tauri-menubar/src/shared/types.ts`
- Create: `tauri-menubar/src/shared/services/helpers.ts`
- Create: `tauri-menubar/src/shared/services/state.ts`
- Create: `tauri-menubar/src/shared/services/dataMapper.ts`
- Create: `tauri-menubar/src/shared/services/auth.ts`

**Step 1: Copy types.ts (only needed interfaces)**

Write `tauri-menubar/src/shared/types.ts` — copy the `ClaudeStatus`, `AgentWindow`, and `AgentSession` interfaces from `web/src/types.ts`. Remove unneeded types (AppTab, TimelineEvent, ConsoleLog, AppSettings, etc.):

```typescript
export interface ClaudeStatus {
    agent_type: 'claude' | 'opencode' | null;
    action: string | null;
    current_tool: string | null;
    model: string | null;
    context_percent: number | null;
    tokens: number | null;
    cost: number | null;
    session_duration: string | null;
    pane: string | null;
}

export interface AgentWindow {
    id: string;
    name: string;
    status: 'IDLE' | 'BUSY' | 'OFFLINE' | 'PAUSED' | 'COMPLETED';
    lastActive: string;
    avatar: string;
    claudeStatus?: ClaudeStatus;
    claudePane?: string;
}

export interface AgentSession {
    id: string;
    name: string;
    status: 'IDLE' | 'BUSY' | 'OFFLINE';
    ip: string;
    windows: AgentWindow[];
    gitDir?: string;
}
```

**Step 2: Copy helpers.ts**

Write `tauri-menubar/src/shared/services/helpers.ts` — exact copy of `web/src/services/helpers.ts`:
```typescript
export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  return `${hours}h${mins}m`;
}

export function formatTime(isoString: string | null): string {
  if (!isoString) return '--:--';
  const date = new Date(isoString);
  return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
}
```

**Step 3: Create auth adapter for Tauri**

Write `tauri-menubar/src/shared/services/auth.ts` — adapted from web version to use Tauri store + hardcoded server URL:

```typescript
import { Store } from '@tauri-apps/plugin-store';

// Hardcoded server URL (no proxy in Tauri)
export const API_BASE = 'http://127.0.0.1:3099/api';
export const WS_BASE = 'ws://127.0.0.1:3099/ws';

const STORE_PATH = 'settings.json';
const AUTH_TOKEN_KEY = 'auth-token';

let _store: Store | null = null;
let _cachedToken: string | null = null;

async function getStore(): Promise<Store> {
  if (!_store) {
    _store = await Store.load(STORE_PATH);
  }
  return _store;
}

export async function getAuthTokenAsync(): Promise<string | null> {
  if (_cachedToken) return _cachedToken;
  const store = await getStore();
  const token = await store.get<string>(AUTH_TOKEN_KEY);
  _cachedToken = token ?? null;
  return _cachedToken;
}

// Synchronous getter (uses cached value)
export function getAuthToken(): string | null {
  return _cachedToken;
}

export async function setAuthToken(token: string): Promise<void> {
  const store = await getStore();
  await store.set(AUTH_TOKEN_KEY, token);
  await store.save();
  _cachedToken = token;
}

export async function clearAuthToken(): Promise<void> {
  const store = await getStore();
  await store.delete(AUTH_TOKEN_KEY);
  await store.save();
  _cachedToken = null;
}

/** Authenticated fetch wrapper */
export async function authFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const token = getAuthToken();
  const headers = new Headers(init?.headers);
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  return fetch(input, { ...init, headers });
}

/** Verify a token against the server */
export async function verifyToken(token: string): Promise<boolean> {
  try {
    const response = await fetch(`${API_BASE}/auth/verify`, {
      headers: { 'Authorization': `Bearer ${token}` },
    });
    return response.ok;
  } catch {
    return false;
  }
}
```

**Step 4: Copy state.ts (WebSocket connection)**

Copy `web/src/services/state.ts` to `tauri-menubar/src/shared/services/state.ts`. Only change imports:

- Change `import { API_BASE, WS_BASE, authFetch, getAuthToken } from './auth';` (same path, now points to Tauri auth adapter)
- Remove unused exports (fetchLogs, fetchNotifications, fetchAlertRules, fetchBackups, etc.) — only keep:
  - `BackendTask`, `BackendState`, `RealtimeMessage`, `StreamChunk`, `ConnectionStatus`
  - `connectWebSocket`, `fetchHealth`

**Step 5: Copy dataMapper.ts (session mapping)**

Copy `web/src/services/dataMapper.ts` to `tauri-menubar/src/shared/services/dataMapper.ts`. Only keep `mapTmuxToSessions`. Update imports:
- Change: `import { AgentSession, AgentWindow } from '../types';` → `import { AgentSession, AgentWindow } from '../types';` (same relative path)
- Change: `import { BackendTask, TmuxWindowInfo, formatDuration } from './api';` → split into correct new locations
- Remove `mapHistoryToTimeline`, `mapTasksToSessions`, `generateConsoleLogs` (unused)

Create `tauri-menubar/src/shared/services/tmux.ts` with just the type:
```typescript
export interface TmuxWindowInfo {
  session_id: string;
  session_name: string;
  window_id: string;
  window_name: string;
  pane_count: number;
  active: boolean;
  git_dir?: string;
}
```

**Step 6: Verify TypeScript compiles**

```bash
cd tauri-menubar && npx tsc --noEmit
```

Expected: No TypeScript errors.

**Step 7: Commit**

```bash
git add tauri-menubar/src/shared/
git commit -m "feat: add shared services — auth adapter, WebSocket, dataMapper, types"
```

---

### Task 4: useTrackerState Hook

**Files:**
- Create: `tauri-menubar/src/hooks/useTrackerState.ts`

**Step 1: Create the hook**

Write `tauri-menubar/src/hooks/useTrackerState.ts`:
```typescript
import { useState, useEffect, useRef, useCallback } from 'react';
import { AgentSession, ClaudeStatus } from '../shared/types';
import { connectWebSocket, RealtimeMessage, ConnectionStatus, fetchHealth } from '../shared/services/state';
import { mapTmuxToSessions } from '../shared/services/dataMapper';
import { getAuthToken, API_BASE, authFetch } from '../shared/services/auth';

interface TrackerState {
  sessions: AgentSession[];
  connectionStatus: ConnectionStatus;
  retryCount: number;
  serverOnline: boolean;
}

interface TrackerStats {
  totalSessions: number;
  busyCount: number;
  idleCount: number;
  totalCost: number;
}

export function useTrackerState() {
  const [state, setState] = useState<TrackerState>({
    sessions: [],
    connectionStatus: 'offline',
    retryCount: 0,
    serverOnline: false,
  });
  const [stats, setStats] = useState<TrackerStats>({
    totalSessions: 0,
    busyCount: 0,
    idleCount: 0,
    totalCost: 0,
  });

  const wsRef = useRef<WebSocket | null>(null);
  const claudeStatusCache = useRef<Map<string, ClaudeStatus>>(new Map());

  // Fetch Claude status for all BUSY windows
  const fetchAllClaudeStatus = useCallback(async (sessions: AgentSession[]) => {
    const token = getAuthToken();
    if (!token) return sessions;

    const busyWindows: { session: AgentSession; window: typeof sessions[0]['windows'][0] }[] = [];
    for (const s of sessions) {
      for (const w of s.windows) {
        if (w.status === 'BUSY' || w.status === 'PAUSED') {
          busyWindows.push({ session: s, window: w });
        }
      }
    }

    await Promise.all(
      busyWindows.map(async ({ session, window: win }) => {
        try {
          const params = new URLSearchParams({ session: session.name, window: win.name });
          const res = await authFetch(`${API_BASE}/tmux/claude-status?${params}`);
          if (res.ok) {
            const data = await res.json();
            if (data.success && data.status) {
              const key = `${session.id}|${win.id}`;
              claudeStatusCache.current.set(key, data.status);
              win.claudeStatus = data.status;
              win.claudePane = data.status.pane || undefined;
            }
          }
        } catch {
          // Ignore individual failures
        }
      })
    );

    // Apply cached status to non-busy windows too
    for (const s of sessions) {
      for (const w of s.windows) {
        if (!w.claudeStatus) {
          const key = `${s.id}|${w.id}`;
          const cached = claudeStatusCache.current.get(key);
          if (cached) w.claudeStatus = cached;
        }
      }
    }

    return sessions;
  }, []);

  // Compute stats
  const computeStats = useCallback((sessions: AgentSession[]): TrackerStats => {
    let totalCost = 0;
    let busyCount = 0;
    let idleCount = 0;

    for (const s of sessions) {
      for (const w of s.windows) {
        if (w.status === 'BUSY') busyCount++;
        else if (w.status === 'IDLE') idleCount++;
        if (w.claudeStatus?.cost) totalCost += w.claudeStatus.cost;
      }
    }

    return {
      totalSessions: sessions.length,
      busyCount,
      idleCount,
      totalCost,
    };
  }, []);

  // Connect WebSocket
  useEffect(() => {
    const token = getAuthToken();
    if (!token) return;

    wsRef.current = connectWebSocket({
      onStateUpdate: async (msg: RealtimeMessage) => {
        const sessions = mapTmuxToSessions(msg.tmux_windows, msg.state.tasks);
        const enriched = await fetchAllClaudeStatus(sessions);
        setState(prev => ({
          ...prev,
          sessions: enriched,
          serverOnline: true,
        }));
        setStats(computeStats(enriched));
      },
      onConnectionChange: (status, retryCount) => {
        setState(prev => ({
          ...prev,
          connectionStatus: status,
          retryCount: retryCount || 0,
          serverOnline: status === 'connected',
        }));
      },
    });

    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [fetchAllClaudeStatus, computeStats]);

  return { ...state, stats };
}
```

**Step 2: Verify TypeScript compiles**

```bash
cd tauri-menubar && npx tsc --noEmit
```

**Step 3: Commit**

```bash
git add tauri-menubar/src/hooks/
git commit -m "feat: add useTrackerState hook — WebSocket + session state + stats"
```

---

### Task 5: LoginView Component

**Files:**
- Create: `tauri-menubar/src/LoginView.tsx`

**Step 1: Create login form**

Write `tauri-menubar/src/LoginView.tsx`:
```tsx
import React, { useState } from 'react';
import { KeyRound, Loader2 } from 'lucide-react';
import { verifyToken, setAuthToken } from './shared/services/auth';

interface LoginViewProps {
  onLogin: () => void;
}

export const LoginView: React.FC<LoginViewProps> = ({ onLogin }) => {
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!token.trim()) return;

    setLoading(true);
    setError('');

    const valid = await verifyToken(token.trim());
    if (valid) {
      await setAuthToken(token.trim());
      onLogin();
    } else {
      setError('Invalid token or server unreachable');
    }
    setLoading(false);
  };

  return (
    <div className="flex flex-col items-center justify-center h-full p-6 bg-neutral-900">
      <KeyRound className="w-8 h-8 text-neutral-400 mb-4" />
      <h2 className="text-sm font-medium text-neutral-200 mb-4">Connect to Agent Tracker</h2>
      <form onSubmit={handleSubmit} className="w-full space-y-3">
        <input
          type="password"
          value={token}
          onChange={e => setToken(e.target.value)}
          placeholder="Auth token"
          className="w-full px-3 py-2 bg-neutral-800 border border-neutral-700 rounded text-sm text-neutral-200 placeholder:text-neutral-500 outline-none focus:border-blue-500"
          autoFocus
        />
        {error && <p className="text-xs text-red-400">{error}</p>}
        <button
          type="submit"
          disabled={loading || !token.trim()}
          className="w-full py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 rounded text-sm text-white font-medium flex items-center justify-center gap-2"
        >
          {loading ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
          Connect
        </button>
      </form>
      <p className="text-[10px] text-neutral-600 mt-4">localhost:3099</p>
    </div>
  );
};
```

**Step 2: Commit**

```bash
git add tauri-menubar/src/LoginView.tsx
git commit -m "feat: add LoginView component for auth token input"
```

---

### Task 6: MenuBarPanel Component

**Files:**
- Create: `tauri-menubar/src/MenuBarPanel.tsx`

**Step 1: Create the panel component**

Write `tauri-menubar/src/MenuBarPanel.tsx`:
```tsx
import React, { useState } from 'react';
import { Monitor, ChevronDown, ChevronRight, Activity, Pause, Check, PowerOff, Pin, Settings, Wifi, WifiOff } from 'lucide-react';
import { AgentSession, AgentWindow, ClaudeStatus } from './shared/types';
import { invoke } from '@tauri-apps/api/core';

interface MenuBarPanelProps {
  sessions: AgentSession[];
  connectionStatus: 'connected' | 'reconnecting' | 'offline';
  stats: {
    totalSessions: number;
    busyCount: number;
    totalCost: number;
  };
}

const STATUS_CONFIG: Record<AgentWindow['status'], { color: string; icon: React.ElementType; label: string }> = {
  IDLE: { color: 'text-green-500', icon: Activity, label: 'IDLE' },
  BUSY: { color: 'text-yellow-400', icon: Activity, label: 'BUSY' },
  PAUSED: { color: 'text-orange-400', icon: Pause, label: 'PAUSED' },
  COMPLETED: { color: 'text-cyan-400', icon: Check, label: 'DONE' },
  OFFLINE: { color: 'text-red-500', icon: PowerOff, label: 'OFF' },
};

const ClaudeInfo: React.FC<{ status: ClaudeStatus }> = ({ status }) => (
  <div className="pl-7 pb-1.5 space-y-0.5">
    {status.current_tool && (
      <div className="text-[11px] text-yellow-400/80 truncate">{status.current_tool}</div>
    )}
    {status.action && !status.current_tool && (
      <div className="text-[11px] text-yellow-400/80 truncate">{status.action}</div>
    )}
    <div className="flex items-center gap-2 text-[10px] text-neutral-500">
      {status.model && <span>{status.model.replace('claude-', '').split('-')[0]}</span>}
      {status.cost != null && <span>${status.cost.toFixed(2)}</span>}
      {status.context_percent != null && <span>{status.context_percent.toFixed(0)}%</span>}
      {status.session_duration && <span>{status.session_duration}</span>}
    </div>
  </div>
);

const WindowRow: React.FC<{ win: AgentWindow }> = ({ win }) => {
  const config = STATUS_CONFIG[win.status];
  return (
    <div className="py-1">
      <div className="flex items-center gap-2 px-3 py-0.5">
        <span className={`text-[10px] ${config.color}`}>{win.status === 'BUSY' ? '●' : win.status === 'IDLE' ? '○' : '◎'}</span>
        <span className="text-[11px] text-neutral-300 truncate flex-1">{win.name}</span>
        <span className={`text-[10px] ${config.color}`}>{config.label}</span>
        {win.lastActive !== '--:--' && (
          <span className="text-[10px] text-neutral-600">{win.lastActive}</span>
        )}
      </div>
      {win.claudeStatus && (win.status === 'BUSY' || win.status === 'PAUSED') && (
        <ClaudeInfo status={win.claudeStatus} />
      )}
    </div>
  );
};

const SessionCard: React.FC<{ session: AgentSession }> = ({ session }) => {
  const [expanded, setExpanded] = useState(session.status === 'BUSY');
  const busyCount = session.windows.filter(w => w.status === 'BUSY').length;
  const sessionColor = busyCount > 0 ? 'text-yellow-400' : 'text-green-500';

  return (
    <div className="border-b border-neutral-800 last:border-b-0">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-2 px-3 py-2 hover:bg-neutral-800/50 transition-colors"
      >
        {expanded ? <ChevronDown className="w-3 h-3 text-neutral-500 shrink-0" /> : <ChevronRight className="w-3 h-3 text-neutral-500 shrink-0" />}
        <Monitor className="w-3.5 h-3.5 text-neutral-400 shrink-0" />
        <span className="text-xs text-neutral-200 font-medium truncate flex-1 text-left">{session.name}</span>
        <span className={`text-[10px] font-medium ${sessionColor}`}>
          {busyCount > 0 ? `${busyCount} BUSY` : 'IDLE'}
        </span>
      </button>
      {expanded && (
        <div className="pb-1">
          {session.windows.map(win => (
            <WindowRow key={win.id} win={win} />
          ))}
        </div>
      )}
    </div>
  );
};

export const MenuBarPanel: React.FC<MenuBarPanelProps> = ({ sessions, connectionStatus, stats }) => {
  const isOnline = connectionStatus === 'connected';

  const handlePinFloat = async () => {
    try {
      await invoke('show_float');
    } catch (e) {
      console.error('Failed to show float:', e);
    }
  };

  return (
    <div className="flex flex-col h-full bg-neutral-900/95 backdrop-blur-xl rounded-lg overflow-hidden border border-neutral-700/50">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-neutral-800">
        <span className="text-xs font-semibold text-neutral-300">Agent Tracker</span>
        <div className="flex items-center gap-1.5">
          {isOnline ? (
            <Wifi className="w-3 h-3 text-green-500" />
          ) : (
            <WifiOff className="w-3 h-3 text-red-500" />
          )}
          <span className={`text-[10px] font-medium ${isOnline ? 'text-green-500' : 'text-red-500'}`}>
            {isOnline ? 'ONLINE' : connectionStatus === 'reconnecting' ? 'RETRY' : 'OFFLINE'}
          </span>
        </div>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto">
        {sessions.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-neutral-600">
            <Monitor className="w-6 h-6 mb-2" />
            <span className="text-xs">{isOnline ? 'No sessions' : 'Disconnected'}</span>
          </div>
        ) : (
          sessions.map(session => (
            <SessionCard key={session.id} session={session} />
          ))
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between px-3 py-1.5 border-t border-neutral-800 bg-neutral-900">
        <button
          onClick={handlePinFloat}
          className="flex items-center gap-1 text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors"
        >
          <Pin className="w-3 h-3" />
          <span>Pin Window</span>
        </button>
        <div className="flex items-center gap-2 text-[10px] text-neutral-600">
          <span>{stats.totalSessions} sessions</span>
          {stats.totalCost > 0 && <span>${stats.totalCost.toFixed(2)}</span>}
        </div>
      </div>
    </div>
  );
};
```

**Step 2: Verify TypeScript compiles**

```bash
cd tauri-menubar && npx tsc --noEmit
```

**Step 3: Commit**

```bash
git add tauri-menubar/src/MenuBarPanel.tsx
git commit -m "feat: add MenuBarPanel — session list with expandable Claude details"
```

---

### Task 7: FloatingBar Component

**Files:**
- Create: `tauri-menubar/src/FloatingBar.tsx`

**Step 1: Create the floating bar component**

Write `tauri-menubar/src/FloatingBar.tsx`:
```tsx
import React from 'react';
import { Hexagon } from 'lucide-react';
import { AgentSession } from './shared/types';

interface FloatingBarProps {
  sessions: AgentSession[];
  stats: {
    totalSessions: number;
    busyCount: number;
    totalCost: number;
  };
  connectionStatus: 'connected' | 'reconnecting' | 'offline';
}

export const FloatingBar: React.FC<FloatingBarProps> = ({ stats, connectionStatus }) => {
  const isOnline = connectionStatus === 'connected';
  const statusColor = !isOnline ? 'text-red-500' : stats.busyCount > 0 ? 'text-yellow-400' : 'text-green-500';

  return (
    <div
      className="flex items-center gap-2 px-3 py-1.5 bg-neutral-900/90 backdrop-blur-xl rounded-lg border border-neutral-700/50 cursor-move select-none"
      data-tauri-drag-region
    >
      <Hexagon className={`w-3.5 h-3.5 ${statusColor} shrink-0`} />
      {isOnline ? (
        <span className="text-[11px] text-neutral-300 whitespace-nowrap">
          {stats.totalSessions} session{stats.totalSessions !== 1 ? 's' : ''}
          {stats.busyCount > 0 && <span className="text-yellow-400"> · {stats.busyCount} busy</span>}
          {stats.totalCost > 0 && <span className="text-neutral-500"> · ${stats.totalCost.toFixed(2)}</span>}
        </span>
      ) : (
        <span className="text-[11px] text-red-400 whitespace-nowrap">
          {connectionStatus === 'reconnecting' ? 'Reconnecting...' : 'Offline'}
        </span>
      )}
    </div>
  );
};
```

**Step 2: Commit**

```bash
git add tauri-menubar/src/FloatingBar.tsx
git commit -m "feat: add FloatingBar — compact always-on-top status bar"
```

---

### Task 8: App Router + Main Entry Point

**Files:**
- Modify: `tauri-menubar/src/main.tsx`
- Create: `tauri-menubar/src/App.tsx`

**Step 1: Create App component with window-based routing**

Write `tauri-menubar/src/App.tsx`:
```tsx
import React, { useState, useEffect } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { MenuBarPanel } from './MenuBarPanel';
import { FloatingBar } from './FloatingBar';
import { LoginView } from './LoginView';
import { useTrackerState } from './hooks/useTrackerState';
import { getAuthTokenAsync } from './shared/services/auth';

export const App: React.FC = () => {
  const [authenticated, setAuthenticated] = useState(false);
  const [loading, setLoading] = useState(true);
  const [windowLabel, setWindowLabel] = useState('panel');

  // Determine which window we're in
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    setWindowLabel(win.label);
  }, []);

  // Check for existing token
  useEffect(() => {
    getAuthTokenAsync().then(token => {
      setAuthenticated(!!token);
      setLoading(false);
    });
  }, []);

  const trackerState = useTrackerState();

  if (loading) {
    return <div className="h-full bg-neutral-900" />;
  }

  if (!authenticated) {
    return <LoginView onLogin={() => setAuthenticated(true)} />;
  }

  if (windowLabel === 'float') {
    return (
      <FloatingBar
        sessions={trackerState.sessions}
        stats={trackerState.stats}
        connectionStatus={trackerState.connectionStatus}
      />
    );
  }

  // Default: panel view
  return (
    <MenuBarPanel
      sessions={trackerState.sessions}
      connectionStatus={trackerState.connectionStatus}
      stats={trackerState.stats}
    />
  );
};
```

**Step 2: Update main.tsx**

Write `tauri-menubar/src/main.tsx`:
```tsx
import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './index.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

**Step 3: Verify TypeScript compiles**

```bash
cd tauri-menubar && npx tsc --noEmit
```

**Step 4: Commit**

```bash
git add tauri-menubar/src/App.tsx tauri-menubar/src/main.tsx
git commit -m "feat: add App router — window-based panel vs float rendering"
```

---

### Task 9: Tray Icon Assets

**Files:**
- Create: `tauri-menubar/src-tauri/icons/icon.png` (32x32 tray template)
- Create: `tauri-menubar/src-tauri/icons/32x32.png`
- Create: `tauri-menubar/src-tauri/icons/128x128.png`
- Create: `tauri-menubar/src-tauri/icons/128x128@2x.png`

**Step 1: Generate icons**

Use the Tauri CLI to generate icons from a source image, or create minimal placeholder icons for development:

```bash
cd tauri-menubar
# If you have a source icon (1024x1024 PNG):
# npx tauri icon path/to/source-icon.png

# For development, create simple placeholder icons using sips (macOS):
# Create a simple monochrome terminal prompt icon
python3 -c "
from PIL import Image, ImageDraw
# 32x32 tray icon (template - black on transparent)
img = Image.new('RGBA', (32, 32), (0,0,0,0))
draw = ImageDraw.Draw(img)
# Simple hexagon shape
points = [(16,2),(28,9),(28,23),(16,30),(4,23),(4,9)]
draw.polygon(points, fill=(0,0,0,255))
img.save('src-tauri/icons/icon.png')
# 128x128
img128 = img.resize((128,128), Image.LANCZOS)
img128.save('src-tauri/icons/128x128.png')
# 256x256
img256 = img.resize((256,256), Image.LANCZOS)
img256.save('src-tauri/icons/128x128@2x.png')
# 32x32
img.save('src-tauri/icons/32x32.png')
print('Icons generated')
"
```

If PIL is not available, use any method to create a 32x32 PNG with a simple monochrome shape for the tray icon, plus larger sizes for the app icon.

Alternatively, temporarily use default Tauri icons:
```bash
cd tauri-menubar
npx tauri icon --help  # Check available options
```

**Step 2: Commit**

```bash
git add tauri-menubar/src-tauri/icons/
git commit -m "feat: add tray and app icons"
```

---

### Task 10: Build, Test, and First Run

**Step 1: Full build**

```bash
cd tauri-menubar
npm install
npm run tauri build -- --debug 2>&1 | tail -30
```

Expected: Build succeeds, produces `.app` bundle.

**Step 2: Run in dev mode**

```bash
cd tauri-menubar
npm run tauri dev
```

Expected:
- Tray icon appears in macOS menu bar
- Click tray → panel window appears below icon
- Panel shows login form (first time) → enter token → shows session list
- "Pin Window" button → floating bar appears

**Step 3: Test key behaviors**

1. Click tray icon → panel shows, click again → panel hides
2. Click outside panel → panel auto-hides (blur event)
3. Enter auth token → sessions appear in real-time
4. BUSY sessions show Claude details (model, cost, context%)
5. "Pin Window" → floating bar appears on top of all windows
6. Floating bar shows summary: "3 sessions · 1 busy · $5.20"
7. Floating bar is draggable

**Step 4: Fix any issues found during testing**

Address any runtime errors, layout issues, or WebSocket connection problems.

**Step 5: Commit**

```bash
git add tauri-menubar/
git commit -m "feat: Tauri menu bar app — working build with tray, panel, and floating bar"
```

---

## Verification Checklist

After all tasks complete:
- [ ] `npm run tauri build` succeeds
- [ ] Tray icon appears in macOS menu bar
- [ ] Click tray toggles panel
- [ ] Panel auto-hides on blur
- [ ] Login form works with tracker-server token
- [ ] Sessions display with real-time status updates via WebSocket
- [ ] BUSY windows show Claude details (model, cost, context%, tool)
- [ ] "Pin Window" opens floating bar
- [ ] Floating bar shows compact stats
- [ ] Floating bar is always-on-top and draggable
- [ ] App has no dock icon (Accessory activation policy)
- [ ] Memory usage under 50MB
