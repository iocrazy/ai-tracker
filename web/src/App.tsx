import React, { useState, useEffect, useCallback, useRef } from 'react';
import { CRTWrapper } from './components/CRTWrapper';
import { WorkstationsView } from './components/WorkstationsView';
import { TimelineView } from './components/TimelineView';
import { ConsoleView } from './components/ConsoleView';
import { SettingsView } from './components/SettingsView';
import { ChatHistoryModal, ChatMessage } from './components/ChatHistoryModal';
import { HistoryDetailModal } from './components/HistoryDetailModal';
import { InputModal } from './components/InputModal';
import { ConfirmationModal } from './components/ConfirmationModal';
import { AddWindowModal, WindowType } from './components/AddWindowModal';
import { CloseWindowModal, CloseAction } from './components/CloseWindowModal';
import { LoginView } from './components/LoginView';
import { AppTab, AppSettings, AgentSession, ConsoleTarget, TimelineEvent, ConsoleLog } from './types';
import { INITIAL_CONSOLE_LOGS } from './constants';
import { Monitor, List, Terminal as TerminalIcon, Settings } from 'lucide-react';
import { fetchState, connectWebSocket, fetchTmuxWindows, tmuxKillSession, tmuxKillWindow, tmuxNewWindow, tmuxSelectWindow, fetchHistoryDetail, fetchClaudeMessages, fetchClaudeStatus, fetchTmuxCapture, BackendState, RealtimeMessage, StreamChunk, ChatMessageEvent, startWorkspace, destroyWorkspace, closeWindow, resumeWorkspace, LayoutType } from './services/api';
import { mapTmuxToSessions, mapHistoryToTimeline, generateConsoleLogs } from './services/dataMapper';

// Helper to generate mock chat history
const generateMockChat = (context: string): ChatMessage[] => {
    const messages: ChatMessage[] = [];
    const count = Math.floor(Math.random() * 5) + 3; // 3-8 messages
    const topics = ["system resource allocation", "deployment sequence", "error log analysis", "network latency check", "security protocol handshake"];
    const topic = topics[Math.floor(Math.random() * topics.length)];

    messages.push({ sender: 'SYSTEM', text: `Connection established to ${context}. Topic: ${topic}`, timestamp: '00:00:01' });
    
    for(let i=0; i<count; i++) {
        messages.push({
            sender: i % 2 === 0 ? 'USER' : 'AGENT',
            text: i % 2 === 0 
                ? `Initiating ${topic} check on sector ${Math.floor(Math.random()*9)}.` 
                : `Acknowledged. Sector ${Math.floor(Math.random()*9)} is reporting nominal status. Efficiency at ${Math.floor(Math.random()*20)+80}%.`,
            timestamp: `00:0${i+1}:4${Math.floor(Math.random()*9)}`
        });
    }
    return messages;
};

const App: React.FC = () => {
  // Auth State (disabled for debugging - set to true to skip login)
  const [isAuthenticated, setIsAuthenticated] = useState(true);
  const [currentUser, setCurrentUser] = useState<string>('heygo');

  const [activeTab, setActiveTab] = useState<AppTab>('WORKSTATIONS');
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const sessionsRef = useRef<AgentSession[]>([]); // Track latest sessions to avoid race conditions
  const [timeline, setTimeline] = useState<TimelineEvent[]>([]);
  const [consoleLogs, setConsoleLogs] = useState<ConsoleLog[]>(INITIAL_CONSOLE_LOGS);
  const [consoleTarget, setConsoleTarget] = useState<ConsoleTarget | null>(null);
  const [isConnected, setIsConnected] = useState(false);
  const [streamOutput, setStreamOutput] = useState<StreamChunk[]>([]);
  const wsRef = useRef<WebSocket | null>(null);

  // Keep sessionsRef in sync with sessions state
  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  // Handle realtime updates from WebSocket (state + tmux_windows in one message)
  // Uses `changed` field to skip re-rendering unchanged sections
  const handleRealtimeUpdate = useCallback((msg: RealtimeMessage) => {
    const changed = msg.state.changed;
    const hasChange = (table: string) => !changed || changed.length === 0 || changed.includes(table);

    // Always update sessions (tmux windows can change without DB changes)
    setSessions(mapTmuxToSessions(msg.tmux_windows, msg.state.tasks));

    // Only update other sections if their backing table changed
    if (hasChange('tasks')) {
      setConsoleLogs(generateConsoleLogs(msg.state));
    }

    setIsConnected(true);
    latestStateRef.current = msg.state;
  }, []);

  // Fetch Claude status for all windows periodically
  // We need claudePane for all windows (not just BUSY) so chat can target the right pane
  useEffect(() => {
    const fetchAllClaudeStatus = async () => {
      // Use sessionsRef to get latest sessions (avoid race condition with WebSocket updates)
      const currentSessions = sessionsRef.current;
      if (currentSessions.length === 0) return;

      // Collect all Claude status updates
      const statusUpdates: Map<string, { claudeStatus?: any; claudePane?: string }> = new Map();

      await Promise.all(
        currentSessions.flatMap((session) =>
          session.windows.map(async (win) => {
            try {
              const response = await fetchClaudeStatus(session.name, win.name);
              if (response.success) {
                const isBusyOrPaused = win.status === 'BUSY' || win.status === 'PAUSED';
                statusUpdates.set(`${session.id}:${win.id}`, {
                  claudeStatus: isBusyOrPaused ? response.status : undefined,
                  claudePane: response.status.pane || undefined,
                });
              }
            } catch {
              // Ignore errors
            }
          })
        )
      );

      // Use functional update to merge status into current state (avoid overwriting WebSocket updates)
      setSessions(prevSessions =>
        prevSessions.map(session => ({
          ...session,
          windows: session.windows.map(win => {
            const update = statusUpdates.get(`${session.id}:${win.id}`);
            if (!update) return win;
            return {
              ...win,
              claudeStatus: update.claudeStatus ?? win.claudeStatus,
              claudePane: update.claudePane ?? win.claudePane,
            };
          }),
        }))
      );
    };

    // Fetch immediately and then every 5 seconds
    fetchAllClaudeStatus();

    const interval = setInterval(fetchAllClaudeStatus, 5000);

    return () => clearInterval(interval);
  }, [sessions.length]); // Only re-run when session count changes

  // Handle stream chunks from WebSocket
  const handleStreamChunk = useCallback((chunk: StreamChunk) => {
    setStreamOutput(prev => [...prev.slice(-100), chunk]); // Keep last 100 chunks
    // Also add to console logs
    setConsoleLogs(prev => [
      ...prev,
      {
        id: `stream-${Date.now()}`,
        type: 'output' as const,
        text: `[${chunk.target}] ${chunk.text}`
      }
    ].slice(-200)); // Keep last 200 logs
  }, []);

  // Track the active session file for WS chat messages
  const activeSessionFileRef = useRef<string>('');

  // Handle real-time chat messages from WebSocket
  const handleChatMessage = useCallback((event: ChatMessageEvent) => {
    // Only update if modal is open and this event matches the active session file
    if (!modalTargetRef.current) return;
    if (activeSessionFileRef.current && event.session_file !== activeSessionFileRef.current) return;

    const newMessages: ChatMessage[] = event.messages.map(m => ({
      sender: m.role === 'user' ? 'USER' : 'AGENT',
      text: m.text,
      timestamp: m.timestamp?.slice(11, 19) || '',
      thinking: m.thinking,
      interaction: m.interaction,
      toolCalls: m.tool_calls,
      toolResults: m.tool_results,
    }));

    if (newMessages.length > 0) {
      setModalMessages(prev => [...prev, ...newMessages]);
    }
  }, []);

  // Store latest state for polling
  const latestStateRef = useRef<BackendState | null>(null);

  // Connect to backend on mount
  useEffect(() => {
    // Initial fetch (fallback if WebSocket is slow to connect)
    Promise.all([fetchState(), fetchTmuxWindows()])
      .then(([state, tmuxWindows]) => {
        latestStateRef.current = state;
        setSessions(mapTmuxToSessions(tmuxWindows, state.tasks));
        setTimeline(mapHistoryToTimeline(state.history));
        setConsoleLogs(generateConsoleLogs(state));
        setIsConnected(true);
      })
      .catch(err => {
        console.error('Failed to fetch initial state:', err);
        setConsoleLogs(prev => [...prev, { id: `err-${Date.now()}`, type: 'system', text: `> ERROR: ${err.message}` }]);
      });

    // WebSocket for real-time updates (state + stream + chat)
    wsRef.current = connectWebSocket({
      onStateUpdate: handleRealtimeUpdate,
      onStreamChunk: handleStreamChunk,
      onChatMessage: handleChatMessage,
    });

    return () => {
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, [handleRealtimeUpdate, handleStreamChunk, handleChatMessage]);
  
  // Modal State
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [modalTitle, setModalTitle] = useState('');
  const [modalSubtitle, setModalSubtitle] = useState('');
  const [modalMessages, setModalMessages] = useState<ChatMessage[]>([]);

  // History Detail Modal
  const [historyDetailId, setHistoryDetailId] = useState<number | null>(null);
  const [historyDetailFilePath, setHistoryDetailFilePath] = useState<string | null>(null);
  const [modalTarget, setModalTarget] = useState<{ session: string; window: string; windowId: string; claudePane?: string } | null>(null);
  const modalTargetRef = useRef<{ session: string; window: string; windowId: string; claudePane?: string } | null>(null);

  // Input Modal State (legacy - for simple window creation)
  const [isInputModalOpen, setIsInputModalOpen] = useState(false);
  const [pendingSessionId, setPendingSessionId] = useState<string | null>(null);

  // Add Window Modal State
  const [addWindowModal, setAddWindowModal] = useState<{ sessionId: string; sessionName: string; gitDir?: string } | null>(null);

  // Close Window Modal State
  const [closeWindowModal, setCloseWindowModal] = useState<{ sessionId: string; sessionName: string; windowId: string; windowName: string; gitDir?: string } | null>(null);

  // Deletion Modal State (for session deletion)
  const [deleteTarget, setDeleteTarget] = useState<{ type: 'SESSION' | 'WINDOW', sessionId: string, windowId?: string, name: string } | null>(null);

  const [settings, setSettings] = useState<AppSettings>(() => {
      const saved = localStorage.getItem('agent-tracker-settings');
      if (saved) {
        try { return JSON.parse(saved); } catch {}
      }
      return {
        theme: 'PHOSPHOR_GREEN',
        scanlines: true,
        flicker: true,
        glow: true,
        noise: false,
        rgbShift: false,
        perspectiveGrid: false,
      };
  });

  // Apply data-theme attribute for modern theme CSS overrides
  useEffect(() => {
    if (settings.theme === 'MODERN') {
      document.documentElement.setAttribute('data-theme', 'modern');
    } else {
      document.documentElement.removeAttribute('data-theme');
    }
  }, [settings.theme]);

  // Persist settings to localStorage
  useEffect(() => {
    localStorage.setItem('agent-tracker-settings', JSON.stringify(settings));
  }, [settings]);

  const handleLogin = (username: string) => {
      setCurrentUser(username);
      setIsAuthenticated(true);
  };

  const updateSetting = (key: keyof AppSettings, value: any) => {
      setSettings(prev => ({ ...prev, [key]: value }));
  };

  const handleRequestAddWindow = (sessionId: string) => {
      const session = sessions.find(s => s.id === sessionId);
      if (session) {
        setAddWindowModal({ sessionId, sessionName: session.name, gitDir: session.gitDir });
      }
  };

  const handleConfirmAddWindow = async (type: WindowType, branchName: string, baseBranch?: string) => {
    if (!addWindowModal) return;

    const { sessionName, gitDir } = addWindowModal;

    try {
      if (type === 'simple') {
        // Simple window - just create tmux window with branch name as window name
        const windowName = branchName || `window-${Date.now()}`;
        const result = await tmuxNewWindow(sessionName, windowName);
        if (!result.success) {
          console.error('Failed to create window:', result.message);
        }
      } else {
        // Worktree-based window - use workspace API
        const layout = type === 'worktree-3pane' ? 'default' : 'workspace';
        const result = await startWorkspace({
          git_dir: gitDir || '',
          branch: branchName,
          base_branch: baseBranch,  // Base branch to create from
          session: sessionName,
          layout,
          auto_open_browser: type === 'worktree-5pane',
        });
        if (!result.success) {
          console.error('Failed to create workspace:', result.message);
        }
      }

      // Refresh tmux data
      const tmuxWindows = await fetchTmuxWindows();
      const state = await fetchState();
      setSessions(mapTmuxToSessions(tmuxWindows, state.tasks));
    } catch (err) {
      console.error('Failed to add window:', err);
    }

    setAddWindowModal(null);
  };

  const handleResumeWindow = async (branchName: string, layout: LayoutType) => {
    if (!addWindowModal) return;

    const { sessionName, gitDir } = addWindowModal;

    try {
      const result = await resumeWorkspace({
        git_dir: gitDir || '',
        branch: branchName,
        session: sessionName,
        layout,
      });
      if (!result.success) {
        console.error('Failed to resume workspace:', result.message);
      }

      // Refresh tmux data
      const tmuxWindows = await fetchTmuxWindows();
      const state = await fetchState();
      setSessions(mapTmuxToSessions(tmuxWindows, state.tasks));
    } catch (err) {
      console.error('Failed to resume window:', err);
    }

    setAddWindowModal(null);
  };

  // Legacy handler for InputModal (kept for compatibility)
  const handleLegacyConfirmAddWindow = async (name: string) => {
    if (!pendingSessionId) return;

    // Find the session name
    const session = sessions.find(s => s.id === pendingSessionId);
    if (!session) {
      setPendingSessionId(null);
      return;
    }

    const result = await tmuxNewWindow(session.name, name);
    if (result.success) {
      // Refresh tmux data
      const tmuxWindows = await fetchTmuxWindows();
      const state = await fetchState();
      setSessions(mapTmuxToSessions(tmuxWindows, state.tasks));
    } else {
      console.error('Failed to create window:', result.message);
    }
    setPendingSessionId(null);
  };

  const handleDeleteSession = async (sessionId: string) => {
      // Find the session name from sessions state
      const session = sessions.find(s => s.id === sessionId);
      if (!session) return;

      const result = await tmuxKillSession(session.name);
      if (result.success) {
        // Refresh tmux data
        const tmuxWindows = await fetchTmuxWindows();
        const state = await fetchState();
        setSessions(mapTmuxToSessions(tmuxWindows, state.tasks));
      } else {
        console.error('Failed to kill session:', result.message);
      }
  };

  const handleDeleteWindow = async (sessionId: string, windowId: string) => {
      // Find the session and window names
      const session = sessions.find(s => s.id === sessionId);
      const window = session?.windows.find(w => w.id === windowId);
      if (!session || !window) return;

      const result = await tmuxKillWindow(session.name, window.name);
      if (result.success) {
        // Refresh tmux data
        const tmuxWindows = await fetchTmuxWindows();
        const state = await fetchState();
        setSessions(mapTmuxToSessions(tmuxWindows, state.tasks));
      } else {
        console.error('Failed to kill window:', result.message);
      }
  };

  const handleRequestDeleteSession = (sessionId: string, name: string) => {
      setDeleteTarget({ type: 'SESSION', sessionId, name });
  };

  const handleRequestDeleteWindow = (sessionId: string, windowId: string, name: string) => {
      // Open the new CloseWindowModal instead of the simple confirmation
      const session = sessions.find(s => s.id === sessionId);
      if (session) {
        setCloseWindowModal({
          sessionId,
          sessionName: session.name,
          windowId,
          windowName: name,
          gitDir: session.gitDir,
        });
      }
  };

  const handleConfirmCloseWindow = async (action: CloseAction, deleteBranch: boolean) => {
      if (!closeWindowModal) return;

      const { sessionName, windowId, windowName, gitDir } = closeWindowModal;

      try {
        if (action === 'close') {
          // Just close the tmux window, keep worktree
          // Use windowId (e.g. @9) for unique identification
          await closeWindow(sessionName, windowId);
        } else {
          // Destroy: delete worktree + tmux window
          // If no gitDir (simple window), just close the tmux window
          if (!gitDir) {
            await closeWindow(sessionName, windowId);
          } else {
            await destroyWorkspace({
              git_dir: gitDir,
              branch: windowName,
              session: sessionName,
              force: true,
              kill_ports: true,
              delete_branch: deleteBranch,
            });
          }
        }

        // Refresh tmux data
        const tmuxWindows = await fetchTmuxWindows();
        const state = await fetchState();
        setSessions(mapTmuxToSessions(tmuxWindows, state.tasks));
      } catch (err) {
        console.error('Failed to close/destroy window:', err);
      }

      setCloseWindowModal(null);
  };

  const handleConfirmDelete = () => {
      if (!deleteTarget) return;

      if (deleteTarget.type === 'SESSION') {
          handleDeleteSession(deleteTarget.sessionId);
      } else if (deleteTarget.type === 'WINDOW' && deleteTarget.windowId) {
          handleDeleteWindow(deleteTarget.sessionId, deleteTarget.windowId);
      }
      setDeleteTarget(null);
  };

  const handleSelectWindow = (sessionName: string, windowName: string, windowId: string) => {
      setConsoleTarget({ session: sessionName, window: windowName, windowId });
      setActiveTab('CONSOLE');
  };

  const handleSwitchToWindow = async (sessionName: string, windowName: string, windowId: string) => {
      const result = await tmuxSelectWindow(sessionName, windowName, windowId);
      if (!result.success) {
          console.error('Failed to switch window:', result.message);
      }
  };

  // Fetch messages for the modal - extracted for reuse in auto-refresh
  const fetchModalMessages = useCallback(async (sessionName: string, windowName: string) => {
    const session = sessions.find(s => s.name === sessionName);
    const win = session?.windows.find(w => w.name === windowName);
    const isActive = win?.status === 'BUSY' || win?.status === 'PAUSED';

    const data = await fetchClaudeMessages(50, { session: sessionName, window: windowName });
    const messages: ChatMessage[] = data.messages.map(m => ({
      sender: m.role === 'user' ? 'USER' : 'AGENT',
      text: m.text,
      timestamp: m.timestamp?.slice(11, 19) || '',
      thinking: m.thinking,
      interaction: m.interaction,
      toolCalls: m.tool_calls,
      toolResults: m.tool_results,
    }));
    return { messages, isActive, sessionFile: data.session_file };
  }, [sessions]);

  // Auto-refresh modal messages every 3 seconds when open
  useEffect(() => {
    if (!isModalOpen || !modalTargetRef.current) return;

    const interval = setInterval(async () => {
      if (!modalTargetRef.current) return;
      try {
        const { session, window } = modalTargetRef.current;
        const { messages, isActive } = await fetchModalMessages(session, window);
        setModalSubtitle(`SOURCE: ${session} // ${isActive ? 'LIVE' : 'ARCHIVE'}`);
        setModalMessages(messages.length > 0 ? messages : [
          { sender: 'SYSTEM', text: 'No conversation history available', timestamp: '' }
        ]);
      } catch (err) {
        console.error('Auto-refresh failed:', err);
      }
    }, 3000);

    return () => clearInterval(interval);
  }, [isModalOpen, fetchModalMessages]);

  const handleViewHistory = async (sessionName: string, windowName: string, windowId: string, claudePane?: string) => {
      setModalTitle(`TRANSCRIPT: ${windowName}`);
      setModalSubtitle(`SOURCE: ${sessionName} // RETRIEVING...`);
      modalTargetRef.current = { session: sessionName, window: windowName, windowId, claudePane };
      setModalTarget({ session: sessionName, window: windowName, windowId, claudePane });
      setIsModalOpen(true);

      try {
        const { messages, isActive, sessionFile } = await fetchModalMessages(sessionName, windowName);
        activeSessionFileRef.current = sessionFile || '';
        setModalSubtitle(`SOURCE: ${sessionName} // ${isActive ? 'LIVE' : 'ARCHIVE'}`);
        setModalMessages(messages.length > 0 ? messages : [
          { sender: 'SYSTEM', text: 'No conversation history available', timestamp: '' }
        ]);
      } catch (err) {
        console.error('Failed to fetch history:', err);
        setModalSubtitle(`SOURCE: ${sessionName} // ERROR`);
        setModalMessages([{ sender: 'SYSTEM', text: 'Failed to load conversation history', timestamp: '' }]);
      }
  };

  const handleTimelineDetails = (event: TimelineEvent) => {
      if (event.filePath) {
        // Session-based: use file path for JSONL detail
        setHistoryDetailFilePath(event.filePath);
        setHistoryDetailId(null);
      } else {
        // Legacy: use history ID
        const id = event.historyId || parseInt(event.id);
        setHistoryDetailId(id);
        setHistoryDetailFilePath(null);
      }
  };

  const renderContent = () => {
      switch (activeTab) {
          case 'WORKSTATIONS': 
            return <WorkstationsView
                        sessions={sessions}
                        onRequestAddWindow={handleRequestAddWindow}
                        onSelectWindow={handleSelectWindow}
                        onSwitchToWindow={handleSwitchToWindow}
                        onRequestDeleteSession={handleRequestDeleteSession}
                        onRequestDeleteWindow={handleRequestDeleteWindow}
                        onViewHistory={handleViewHistory}
                   />;
          case 'TIMELINE':
            return <TimelineView
                        events={timeline}
                        onViewDetails={handleTimelineDetails}
                        isActive={!isModalOpen && !isInputModalOpen && !deleteTarget}
                   />;
          case 'CONSOLE':
            return <ConsoleView logs={consoleLogs} target={consoleTarget} />;
          case 'SETTINGS': 
            return <SettingsView settings={settings} onUpdate={updateSetting} />;
          default: return null;
      }
  };

  return (
    <CRTWrapper settings={settings}>
      {/* If not authenticated, show LoginView. Otherwise show Main App */}
      {!isAuthenticated ? (
          <LoginView onLogin={handleLogin} />
      ) : (
          <div className="flex flex-col h-[100dvh] max-w-[1600px] mx-auto overflow-hidden">

            {/* Header - Compact on mobile, full on desktop */}
            <header className="flex-none justify-between items-center py-2 md:py-4 lg:py-6 mb-1 md:mb-2 flex px-2 md:px-4">
                <div className="flex items-center gap-2 md:gap-4">
                     <div className="flex flex-col md:flex-row md:items-baseline gap-1 md:gap-2">
                        <h1 className="text-lg sm:text-xl md:text-2xl lg:text-4xl font-black text-green-500 tracking-tight retro-text-shadow uppercase font-pixel">
                            AGENT<span className="bg-green-900/20 px-1 sm:px-2 ml-1 border border-green-800/50 rounded text-green-400 shadow-[0_0_10px_rgba(34,197,94,0.3)] text-base sm:text-lg md:text-xl lg:text-3xl">TRACKER</span>
                        </h1>
                     </div>
                </div>

                <div className="flex flex-col items-end gap-0.5 md:gap-1">
                    <div className="flex items-center gap-1.5 md:gap-3 bg-black/40 px-2 md:px-4 py-1 md:py-2 border border-green-900 rounded-full">
                        <div className={`w-3 h-3 rounded-full ${isConnected ? 'bg-green-500 shadow-[0_0_8px_#22c55e] animate-pulse' : 'bg-yellow-500 shadow-[0_0_8px_#eab308] animate-pulse'}`}></div>
                        <span className={`hidden sm:inline font-bold tracking-wider md:tracking-widest text-xs md:text-sm lg:text-lg retro-text-shadow ${isConnected ? 'text-green-400' : 'text-yellow-400'}`}>
                            {isConnected ? 'ONLINE' : 'CONNECTING...'}
                        </span>
                    </div>
                    {/* Logged in user display - hidden on small screens */}
                    <div className="hidden sm:flex text-green-800 font-mono text-xs tracking-widest items-center gap-2">
                        <span className="w-1.5 h-1.5 bg-green-700 rounded-full"></span>
                        OP: {currentUser.toUpperCase()}
                    </div>
                </div>
            </header>

            {/* Desktop Navigation Tabs - Hidden on mobile/tablet (below 1280px) */}
            <nav className="hidden xl:flex flex-row flex-none flex-wrap gap-1 mb-4 border-b-2 border-green-600 shadow-[0_5px_15px_rgba(34,197,94,0.1)]">
                {[
                    { id: 'WORKSTATIONS', icon: Monitor, label: 'Workstations' },
                    { id: 'TIMELINE', icon: List, label: 'Timeline' },
                    { id: 'CONSOLE', icon: TerminalIcon, label: 'Console' },
                    { id: 'SETTINGS', icon: Settings, label: 'Settings' },
                ].map((tab) => (
                    <button
                        key={tab.id}
                        onClick={() => setActiveTab(tab.id as AppTab)}
                        className={`
                            flex items-center gap-2 px-4 py-3 font-bold text-xs tracking-widest uppercase transition-all font-pixel
                            ${activeTab === tab.id
                                ? 'bg-green-600 text-black shadow-[0_0_25px_rgba(34,197,94,0.6)] z-10'
                                : 'bg-black/40 text-green-700 hover:text-green-400 hover:bg-green-900/20'
                            }
                        `}
                    >
                        <tab.icon className={`w-4 h-4 ${activeTab === tab.id ? 'text-black' : 'text-green-600'}`} />
                        {tab.label}
                    </button>
                ))}
            </nav>

            {/* Main Content Area - Add bottom padding for fixed nav on mobile */}
            <main className="flex-1 overflow-y-auto min-h-0 animate-[fadeIn_0.3s_ease-out] pb-24 xl:pb-10 px-2 md:px-4">
                {renderContent()}
            </main>

            {/* Mobile/Tablet Bottom Navigation - Fixed at bottom, shown below 1280px */}
            <nav className="xl:hidden fixed bottom-0 left-0 right-0 z-50 bg-black border-t-2 border-green-600 shadow-[0_-5px_20px_rgba(34,197,94,0.2)]" style={{ paddingBottom: 'max(8px, env(safe-area-inset-bottom, 8px))' }}>
                <div className="flex justify-around items-stretch max-w-[600px] mx-auto">
                    {[
                        { id: 'WORKSTATIONS', icon: Monitor, label: 'WORK' },
                        { id: 'TIMELINE', icon: List, label: 'TIMELINE' },
                        { id: 'CONSOLE', icon: TerminalIcon, label: 'CONSOLE' },
                        { id: 'SETTINGS', icon: Settings, label: 'SETTINGS' },
                    ].map((tab) => (
                        <button
                            key={tab.id}
                            onClick={() => setActiveTab(tab.id as AppTab)}
                            className={`
                                flex-1 flex flex-col items-center justify-center gap-1.5 py-4 md:py-5 transition-all
                                ${activeTab === tab.id
                                    ? 'bg-green-900/30 text-green-400 border-t-2 border-green-400 -mt-[2px]'
                                    : 'text-green-700 hover:text-green-500 hover:bg-green-900/10'
                                }
                            `}
                        >
                            <tab.icon className={`w-5 h-5 md:w-6 md:h-6 ${activeTab === tab.id ? 'text-green-400' : 'text-green-600'}`} />
                            <span className="text-[8px] md:text-[10px] font-bold tracking-wider font-pixel">{tab.label}</span>
                        </button>
                    ))}
                </div>
            </nav>
            
            {/* Chat Modal Layer */}
            <ChatHistoryModal
                isOpen={isModalOpen}
                onClose={() => {
                  setIsModalOpen(false);
                  modalTargetRef.current = null;
                  activeSessionFileRef.current = '';
                  setModalTarget(null);
                }}
                title={modalTitle}
                subtitle={modalSubtitle}
                messages={modalMessages}
                sessionName={modalTarget?.session}
                windowName={modalTarget?.window}
                windowId={modalTarget?.windowId}
                claudePane={modalTarget?.claudePane}
                claudeStatus={
                  modalTarget
                    ? sessions
                        .find(s => s.name === modalTarget.session)
                        ?.windows.find(w => w.id === modalTarget.windowId)
                        ?.claudeStatus
                    : undefined
                }
            />

            {/* History Detail Modal */}
            <HistoryDetailModal
                historyId={historyDetailId || 0}
                filePath={historyDetailFilePath || undefined}
                isOpen={historyDetailId !== null || historyDetailFilePath !== null}
                onClose={() => { setHistoryDetailId(null); setHistoryDetailFilePath(null); }}
            />

            {/* Add Window Modal */}
            {addWindowModal && (
              <AddWindowModal
                sessionName={addWindowModal.sessionName}
                gitDir={addWindowModal.gitDir}
                openWindows={sessions.find(s => s.id === addWindowModal.sessionId)?.windows.map(w => w.name) || []}
                onClose={() => setAddWindowModal(null)}
                onConfirm={handleConfirmAddWindow}
                onResume={handleResumeWindow}
              />
            )}

            {/* Close Window Modal */}
            {closeWindowModal && (
              <CloseWindowModal
                sessionName={closeWindowModal.sessionName}
                windowName={closeWindowModal.windowName}
                hasWorktree={!!closeWindowModal.gitDir}
                onClose={() => setCloseWindowModal(null)}
                onConfirm={handleConfirmCloseWindow}
              />
            )}

            {/* Legacy Input Modal Layer */}
            <InputModal
                isOpen={isInputModalOpen}
                onClose={() => setIsInputModalOpen(false)}
                onSubmit={handleLegacyConfirmAddWindow}
                title="INITIALIZE_WORKSTATION"
                placeholder="ENTER_WORKTREE_ID..."
            />

            {/* Confirmation Modal Layer (for session deletion) */}
            <ConfirmationModal
                isOpen={!!deleteTarget}
                onClose={() => setDeleteTarget(null)}
                onConfirm={handleConfirmDelete}
                title="DELETION_PROTOCOL_INITIATED"
                message={`WARNING: PERMANENTLY REMOVE ${deleteTarget?.type} "${deleteTarget?.name}"? THIS ACTION CANNOT BE REVERSED.`}
            />
          </div>
      )}
    </CRTWrapper>
  );
};

export default App;