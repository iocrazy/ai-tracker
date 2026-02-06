import React, { useState } from 'react';
import { CRTWrapper } from './components/CRTWrapper';
import { WorkstationsView } from './components/WorkstationsView';
import { TimelineView } from './components/TimelineView';
import { ConsoleView } from './components/ConsoleView';
import { SettingsView } from './components/SettingsView';
import { ChatHistoryModal, ChatMessage } from './components/ChatHistoryModal';
import { InputModal } from './components/InputModal';
import { ConfirmationModal } from './components/ConfirmationModal';
import { LoginView } from './components/LoginView';
import { AppTab, AppSettings, AgentSession, ConsoleTarget, TimelineEvent } from './types';
import { MOCK_SESSIONS, MOCK_TIMELINE, INITIAL_CONSOLE_LOGS } from './constants';
import { Monitor, List, Terminal as TerminalIcon, Settings } from 'lucide-react';

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
  // Auth State
  const [isAuthenticated, setIsAuthenticated] = useState(false);
  const [currentUser, setCurrentUser] = useState<string>('');

  const [activeTab, setActiveTab] = useState<AppTab>('WORKSTATIONS');
  const [sessions, setSessions] = useState<AgentSession[]>(MOCK_SESSIONS);
  const [consoleTarget, setConsoleTarget] = useState<ConsoleTarget | null>(null);
  
  // Modal State
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [modalTitle, setModalTitle] = useState('');
  const [modalSubtitle, setModalSubtitle] = useState('');
  const [modalMessages, setModalMessages] = useState<ChatMessage[]>([]);

  // Input Modal State
  const [isInputModalOpen, setIsInputModalOpen] = useState(false);
  const [pendingSessionId, setPendingSessionId] = useState<string | null>(null);

  // Deletion Modal State
  const [deleteTarget, setDeleteTarget] = useState<{ type: 'SESSION' | 'WINDOW', sessionId: string, windowId?: string, name: string } | null>(null);

  const [settings, setSettings] = useState<AppSettings>({
      theme: 'PHOSPHOR_GREEN',
      scanlines: true,
      flicker: true,
      glow: true,
      noise: false,
      rgbShift: false,
      perspectiveGrid: false
  });

  const handleLogin = (username: string) => {
      setCurrentUser(username);
      setIsAuthenticated(true);
  };

  const updateSetting = (key: keyof AppSettings, value: any) => {
      setSettings(prev => ({ ...prev, [key]: value }));
  };

  const handleRequestAddWindow = (sessionId: string) => {
      setPendingSessionId(sessionId);
      setIsInputModalOpen(true);
  };

  const handleConfirmAddWindow = (name: string) => {
    if (!pendingSessionId) return;
    
    setSessions(prev => prev.map(session => {
        if (session.id === pendingSessionId) {
            return {
                ...session,
                windows: [
                    ...session.windows,
                    {
                        id: `w-${Date.now()}`,
                        name: name,
                        status: 'IDLE',
                        lastActive: 'Just now',
                        avatar: `https://api.dicebear.com/7.x/avataaars/svg?seed=${name}`
                    }
                ]
            };
        }
        return session;
    }));
    setPendingSessionId(null);
  };

  const handleDeleteSession = (sessionId: string) => {
      setSessions(prev => prev.filter(s => s.id !== sessionId));
  };

  const handleDeleteWindow = (sessionId: string, windowId: string) => {
      setSessions(prev => prev.map(session => {
          if (session.id === sessionId) {
              return {
                  ...session,
                  windows: session.windows.filter(w => w.id !== windowId)
              };
          }
          return session;
      }));
  };

  const handleRequestDeleteSession = (sessionId: string, name: string) => {
      setDeleteTarget({ type: 'SESSION', sessionId, name });
  };

  const handleRequestDeleteWindow = (sessionId: string, windowId: string, name: string) => {
      setDeleteTarget({ type: 'WINDOW', sessionId, windowId, name });
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

  const handleSelectWindow = (sessionName: string, windowName: string) => {
      setConsoleTarget({ session: sessionName, window: windowName });
      setActiveTab('CONSOLE');
  };

  const handleViewHistory = (sessionName: string, windowName: string) => {
      setModalTitle(`TRANSCRIPT: ${windowName}`);
      setModalSubtitle(`SOURCE: ${sessionName} // ARCHIVE_RETRIEVAL_OK`);
      setModalMessages(generateMockChat(`${sessionName}::${windowName}`));
      setIsModalOpen(true);
  };

  const handleTimelineDetails = (event: TimelineEvent) => {
      setModalTitle(`EVENT LOG: ${event.id}`);
      setModalSubtitle(`USER: ${event.user} // ACTION: ${event.action} // TIME: ${event.time}`);
      setModalMessages(generateMockChat(`Timeline Event ${event.id}`));
      setIsModalOpen(true);
  };

  const renderContent = () => {
      switch (activeTab) {
          case 'WORKSTATIONS': 
            return <WorkstationsView 
                        sessions={sessions} 
                        onRequestAddWindow={handleRequestAddWindow} 
                        onSelectWindow={handleSelectWindow}
                        onRequestDeleteSession={handleRequestDeleteSession}
                        onRequestDeleteWindow={handleRequestDeleteWindow}
                        onViewHistory={handleViewHistory}
                   />;
          case 'TIMELINE': 
            return <TimelineView 
                        events={MOCK_TIMELINE} 
                        onViewDetails={handleTimelineDetails} 
                        isActive={!isModalOpen && !isInputModalOpen && !deleteTarget} 
                   />;
          case 'CONSOLE': 
            return <ConsoleView logs={INITIAL_CONSOLE_LOGS} target={consoleTarget} />;
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
          <div className="flex flex-col h-full max-w-[1600px] mx-auto pb-10 min-h-screen">
            
            {/* Header */}
            <header className="flex-none justify-between items-center py-6 mb-2 flex">
                <div className="flex items-center gap-4">
                     <div className="flex flex-col md:flex-row md:items-baseline gap-2">
                        <h1 className="text-6xl md:text-8xl font-black text-green-500 tracking-tighter retro-text-shadow uppercase" style={{fontFamily: 'VT323'}}>
                            AGENT<span className="bg-green-900/20 px-4 ml-2 border border-green-800/50 rounded text-green-400 shadow-[0_0_10px_rgba(34,197,94,0.3)]">TRACKER</span>
                        </h1>
                        <span className="text-green-800 font-mono text-xl tracking-widest ml-2">v0.1.0</span>
                     </div>
                </div>
                
                <div className="flex flex-col items-end gap-1">
                    <div className="flex items-center gap-3 bg-black/40 px-4 py-2 border border-green-900 rounded-full">
                        <div className="w-3 h-3 bg-green-500 rounded-full animate-pulse shadow-[0_0_8px_#22c55e]"></div>
                        <span className="text-green-400 font-bold tracking-widest text-lg retro-text-shadow">SYSTEM_ONLINE</span>
                    </div>
                    {/* Logged in user display */}
                    <div className="text-green-800 font-mono text-xs tracking-widest flex items-center gap-2">
                        <span className="w-1.5 h-1.5 bg-green-700 rounded-full"></span>
                        OP: {currentUser.toUpperCase()}
                    </div>
                </div>
            </header>

            {/* Navigation Tabs */}
            <nav className="flex-none flex flex-wrap gap-1 mb-8 border-b-2 border-green-600 shadow-[0_5px_15px_rgba(34,197,94,0.1)]">
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
                            flex items-center gap-4 px-8 py-5 font-bold text-3xl tracking-widest uppercase transition-all
                            ${activeTab === tab.id 
                                ? 'bg-green-600 text-black shadow-[0_0_25px_rgba(34,197,94,0.6)] z-10' 
                                : 'bg-black/40 text-green-700 hover:text-green-400 hover:bg-green-900/20'
                            }
                        `}
                    >
                        <tab.icon className={`w-8 h-8 ${activeTab === tab.id ? 'text-black' : 'text-green-600'}`} />
                        {tab.label}
                    </button>
                ))}
            </nav>

            {/* Main Content Area */}
            <main className="flex-grow overflow-y-auto min-h-0 animate-[fadeIn_0.3s_ease-out]">
                {renderContent()}
            </main>
            
            {/* Modal Layer */}
            <ChatHistoryModal 
                isOpen={isModalOpen}
                onClose={() => setIsModalOpen(false)}
                title={modalTitle}
                subtitle={modalSubtitle}
                messages={modalMessages}
            />

            {/* Input Modal Layer */}
            <InputModal
                isOpen={isInputModalOpen}
                onClose={() => setIsInputModalOpen(false)}
                onSubmit={handleConfirmAddWindow}
                title="INITIALIZE_WORKSTATION"
                placeholder="ENTER_WORKTREE_ID..."
            />

            {/* Confirmation Modal Layer */}
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