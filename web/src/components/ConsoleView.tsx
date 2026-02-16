import React, { useState, useRef, useEffect } from 'react';
import { ConsoleLog, ConsoleTarget } from '../types';
import { Command, ChevronRight, TerminalSquare, ScrollText, Search, RefreshCw } from 'lucide-react';
import { executeTmuxCommand } from '../services/api';
import { fetchLogs, LogEntry } from '../services/state';

interface ConsoleViewProps {
    logs: ConsoleLog[];
    target: ConsoleTarget | null;
}

const TMUX_TEMPLATES = [
    {
        id: 'send_keys',
        label: 'Send Keys',
        cmd: 'tmux send-keys -t {session}:{window}.{pane} "{keys}" C-m',
        desc: 'Send text + Enter'
    },
    {
        id: 'split_h',
        label: 'Split Horizontal',
        cmd: 'tmux split-window -h -t {session}:{window}',
        desc: 'Split window horizontally'
    },
    { 
        id: 'split_v', 
        label: 'Split Vertical', 
        cmd: 'tmux split-window -v -t {session}:{window}',
        desc: 'Split window vertically'
    },
    { 
        id: 'select_pane', 
        label: 'Select Pane', 
        cmd: 'tmux select-pane -t {session}:{window}.{pane}',
        desc: 'Switch focus to pane'
    },
    { 
        id: 'resize', 
        label: 'Resize Pane', 
        cmd: 'tmux resize-pane -t {session}:{window}.{pane} -D 5',
        desc: 'Resize pane down by 5'
    },
    { 
        id: 'new_win', 
        label: 'New Window', 
        cmd: 'tmux new-window -t {session} -n {name}',
        desc: 'Create window in session'
    },
    { 
        id: 'kill_sess', 
        label: 'Kill Session', 
        cmd: 'tmux kill-session -t {session}',
        desc: 'Terminate target session'
    }
];

export const ConsoleView: React.FC<ConsoleViewProps> = ({ logs: initialLogs, target }) => {
  const [logs, setLogs] = useState<ConsoleLog[]>(initialLogs);
  const [input, setInput] = useState('');

  // Template Variable State
  const [sessionInput, setSessionInput] = useState('');
  const [windowInput, setWindowInput] = useState('');      // Display name
  const [windowIdInput, setWindowIdInput] = useState('');  // Actual ID for tmux targeting
  const [paneInput, setPaneInput] = useState('');
  const [keysInput, setKeysInput] = useState('');

  // Console mode: 'tmux' or 'logs'
  const [consoleMode, setConsoleMode] = useState<'tmux' | 'logs'>('tmux');

  // Server logs state
  const [serverLogs, setServerLogs] = useState<LogEntry[]>([]);
  const [logLevel, setLogLevel] = useState<string>('');
  const [logSearch, setLogSearch] = useState('');
  const [logLoading, setLogLoading] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);

  // Load server logs
  const loadServerLogs = async () => {
    setLogLoading(true);
    const result = await fetchLogs({ limit: 200, level: logLevel || undefined, search: logSearch || undefined });
    setServerLogs(result.entries);
    setLogLoading(false);
  };

  useEffect(() => {
    if (consoleMode === 'logs') loadServerLogs();
  }, [consoleMode, logLevel]);

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs]);

  useEffect(() => {
    if (target) {
        setSessionInput(target.session);
        setWindowInput(target.window);
        setWindowIdInput(target.windowId || target.window);  // Fallback to name if no ID
        setLogs(prev => [...prev, {
            id: Date.now().toString(),
            type: 'system',
            text: `> Connected to target: ${target.session} :: ${target.window}`
        }]);
    }
  }, [target]);

  // Removed auto-focus to prevent keyboard from opening on mobile

  const handleSend = async () => {
    if (!input.trim()) return;

    const command = input;
    const newLog: ConsoleLog = {
        id: Date.now().toString(),
        type: 'input',
        text: `> ${command}`
    };

    setLogs(prev => [...prev, newLog]);
    setInput('');

    // Execute the command via API
    try {
      const result = await executeTmuxCommand(command);
      setLogs(prev => [...prev, {
        id: (Date.now() + 1).toString(),
        type: result.success ? 'output' : 'system',
        text: result.success ? `✓ ${result.message}` : `✗ ${result.message}`
      }]);
    } catch (err) {
      setLogs(prev => [...prev, {
        id: (Date.now() + 1).toString(),
        type: 'system',
        text: `✗ Error: ${err instanceof Error ? err.message : 'Unknown error'}`
      }]);
    }
  };

  const applyTemplate = (templateCmd: string) => {
      let cmd = templateCmd;

      // Auto-fill variables from inputs
      // Use windowIdInput for tmux targeting (falls back to windowInput if not set)
      cmd = cmd.replace(/{session}/g, sessionInput || 'current_session');
      cmd = cmd.replace(/{window}/g, windowIdInput || windowInput || 'current_window');

      // Handle pane - if empty, remove the .{pane} suffix entirely
      if (paneInput) {
          cmd = cmd.replace(/{pane}/g, paneInput);
      } else {
          // Remove .{pane} when pane is not specified (targets active pane)
          cmd = cmd.replace(/\.\{pane\}/g, '');
      }

      // Auto-fill keys
      if (keysInput) {
          cmd = cmd.replace(/{keys}/g, keysInput);
      }

      setInput(cmd);
      inputRef.current?.focus();
  };

  const inputClass = "bg-black border border-green-500/50 px-3 py-2 text-lg text-green-300 font-bold shadow-[0_0_8px_rgba(34,197,94,0.2)] focus:outline-none focus:border-green-400 placeholder-green-900/50 font-mono transition-all";

  return (
    <div className="flex flex-col h-full gap-4 pb-2">
        {/* Mode Toggle */}
        <div className="flex items-center gap-1 flex-none">
          <button onClick={() => setConsoleMode('tmux')}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-[10px] font-bold tracking-widest uppercase transition-all border
              ${consoleMode === 'tmux' ? 'text-green-300 border-green-500 bg-green-900/30' : 'text-green-700 border-green-900 hover:border-green-700'}`}>
            <TerminalSquare className="w-3 h-3" /> TMUX CONSOLE
          </button>
          <button onClick={() => setConsoleMode('logs')}
            className={`flex items-center gap-1.5 px-3 py-1.5 text-[10px] font-bold tracking-widest uppercase transition-all border
              ${consoleMode === 'logs' ? 'text-green-300 border-green-500 bg-green-900/30' : 'text-green-700 border-green-900 hover:border-green-700'}`}>
            <ScrollText className="w-3 h-3" /> SERVER LOGS
          </button>
        </div>

        {/* Server Logs Mode */}
        {consoleMode === 'logs' ? (
          <div className="flex-grow flex flex-col min-h-0 retro-border bg-black/80 p-4 relative overflow-hidden">
            {/* Log filters */}
            <div className="flex items-center gap-2 mb-3 flex-wrap flex-none">
              <div className="flex items-center gap-1">
                {['', 'INFO', 'WARN', 'ERROR', 'DEBUG'].map(l => (
                  <button key={l} onClick={() => setLogLevel(l)}
                    className={`px-2 py-1 text-[9px] font-bold tracking-widest uppercase transition-all border
                      ${logLevel === l ? 'text-green-300 border-green-500 bg-green-900/30' : 'text-green-700 border-green-900 hover:border-green-700'}`}>
                    {l || 'ALL'}
                  </button>
                ))}
              </div>
              <div className="flex items-center gap-1 flex-1 min-w-[150px]">
                <Search className="w-3 h-3 text-green-700" />
                <input value={logSearch} onChange={e => setLogSearch(e.target.value)}
                  onKeyDown={e => e.key === 'Enter' && loadServerLogs()}
                  placeholder="Filter logs..."
                  className="flex-1 bg-black/60 border border-green-900 text-green-300 px-2 py-1 text-xs font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
              </div>
              <button onClick={loadServerLogs}
                className="flex items-center gap-1 px-2 py-1 border border-green-900 text-green-600 hover:border-green-500 hover:text-green-400 transition-all">
                <RefreshCw className={`w-3 h-3 ${logLoading ? 'animate-spin' : ''}`} />
              </button>
            </div>

            {/* Log entries */}
            <div className="flex-grow overflow-y-auto custom-scrollbar font-mono text-xs space-y-0">
              {serverLogs.length === 0 && !logLoading ? (
                <div className="flex items-center justify-center py-8 text-green-700 text-sm">No log entries found</div>
              ) : (
                serverLogs.map((entry, i) => (
                  <div key={i} className="flex items-start gap-2 py-0.5 border-b border-green-900/10 hover:bg-green-900/5">
                    <span className="text-green-800 shrink-0 w-[85px]">{entry.timestamp.slice(11, 23)}</span>
                    <span className={`shrink-0 w-[42px] font-bold ${
                      entry.level === 'ERROR' ? 'text-red-500' :
                      entry.level === 'WARN' ? 'text-yellow-500' :
                      entry.level === 'DEBUG' ? 'text-blue-500' :
                      'text-green-600'
                    }`}>{entry.level}</span>
                    <span className="text-green-700 shrink-0 max-w-[180px] truncate">{entry.module}</span>
                    <span className="text-green-400 break-all">{entry.message}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        ) : (
        <>
        {/* 1. Target Selectors Header */}
        <div className="flex flex-col sm:flex-row sm:flex-wrap sm:items-center gap-3 sm:gap-4 text-green-500 font-mono uppercase p-3 sm:p-4 border border-green-900/50 bg-black/40 flex-none">
            {/* Title + Status indicator row */}
            <div className="flex items-center justify-between sm:justify-start gap-2">
                <span className="font-bold tracking-widest text-green-500 text-sm sm:text-base">CONNECTION:</span>
                <div className={`w-4 h-4 rounded-full sm:hidden ${sessionInput && windowInput ? 'bg-green-500 animate-pulse shadow-[0_0_8px_#22c55e]' : 'bg-red-900'}`}></div>
            </div>

            {/* SESSION / WINDOW / PANE - Grid on mobile, flex on desktop */}
            <div className="grid grid-cols-3 gap-2 sm:flex sm:items-center sm:gap-4">
                <div className="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                    <span className="text-green-800 text-[10px] sm:text-xs">SESSION</span>
                    <input
                        type="text"
                        value={sessionInput}
                        onChange={(e) => setSessionInput(e.target.value)}
                        placeholder="SESSION"
                        className={`${inputClass} w-full sm:w-[160px] text-sm sm:text-lg py-2`}
                    />
                </div>

                <div className="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                    <span className="text-green-800 text-[10px] sm:text-xs">WINDOW</span>
                    <input
                        type="text"
                        value={windowInput}
                        onChange={(e) => setWindowInput(e.target.value)}
                        placeholder="WINDOW"
                        className={`${inputClass} w-full sm:w-[120px] text-sm sm:text-lg py-2`}
                    />
                </div>

                <div className="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                    <span className="text-green-800 text-[10px] sm:text-xs">PANE</span>
                    <input
                        type="text"
                        value={paneInput}
                        onChange={(e) => setPaneInput(e.target.value)}
                        placeholder="#"
                        className={`${inputClass} w-full sm:w-[70px] text-center text-sm sm:text-lg py-2`}
                    />
                </div>
            </div>

            {/* Keys Input - Full width row */}
            <div className="flex items-center gap-2 w-full sm:flex-grow sm:w-auto sm:min-w-[200px]">
                <span className="text-green-800 text-[10px] sm:text-sm whitespace-nowrap">KEYS</span>
                <input
                    type="text"
                    value={keysInput}
                    onChange={(e) => setKeysInput(e.target.value)}
                    placeholder="Value for {keys}..."
                    className={`${inputClass} flex-1 text-sm sm:text-lg py-2`}
                />
            </div>

            {/* Desktop status indicator */}
            <div className="hidden sm:block ml-auto pl-4 border-l border-green-900/30">
                 <div className={`w-4 h-4 rounded-full ${sessionInput && windowInput ? 'bg-green-500 animate-pulse shadow-[0_0_8px_#22c55e]' : 'bg-red-900'}`}></div>
            </div>
        </div>

        {/* 2. Main Content: Terminal & Input (SWAPPED to TOP) */}
        <div 
            className="flex-grow flex flex-col min-h-0 retro-border bg-black/80 p-6 relative overflow-hidden shadow-[inset_0_0_50px_rgba(0,0,0,0.5)] cursor-text"
            onClick={() => inputRef.current?.focus()}
        >
             <div className="flex items-center gap-2 sm:gap-3 text-green-500 border-b border-green-900/50 pb-2 mb-2 flex-none">
                <TerminalSquare className="w-4 sm:w-5 h-4 sm:h-5" />
                <span className="font-bold tracking-widest text-[10px] sm:text-xs uppercase font-pixel">LIVE SESSION LOG</span>
            </div>

            {/* Scrollable Logs */}
            <div className="flex-grow overflow-y-auto custom-scrollbar font-mono text-base sm:text-xl space-y-1 p-1 sm:p-2">
                 {logs.map((log, i) => (
                    <div key={`${log.id}-${i}`} className={`break-words ${
                        log.type === 'input' ? 'text-green-300 font-bold' :
                        log.type === 'system' ? 'text-green-800 italic' :
                        'text-green-500'
                    }`}>
                        {log.text}
                    </div>
                 ))}
                 <div ref={logsEndRef}></div>
            </div>

            {/* Input Line */}
            <div className="flex-none flex items-center gap-2 sm:gap-3 relative border-t border-green-900/50 pt-2 sm:pt-4 mt-2">
                <span className="text-green-500 font-bold text-xl sm:text-3xl animate-pulse select-none flex items-center h-full pb-1">{'>'}</span>

                <form onSubmit={(e) => { e.preventDefault(); handleSend(); }} className="flex-grow flex items-center">
                    <input
                        ref={inputRef}
                        type="text"
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        className="w-full bg-transparent border-none outline-none text-green-300 font-bold text-lg sm:text-2xl p-0 focus:ring-0 placeholder-green-900/30 font-mono caret-green-500 h-8 sm:h-10"
                        autoComplete="off"
                        placeholder="ENTER COMMAND..."
                    />
                </form>

                <button
                    onClick={handleSend}
                    className="hidden md:flex items-center gap-2 px-3 py-1 bg-green-900/10 border border-green-500/50 rounded text-green-400 text-sm font-bold tracking-widest hover:bg-green-900/30 hover:border-green-400 transition-all cursor-pointer"
                >
                    <span>EXECUTE</span>
                    <span className="border border-green-500/50 px-1 rounded bg-black/30">↵</span>
                </button>
            </div>
        </div>

        {/* 3. Bottom: Templates (SWAPPED to BOTTOM) */}
        <div className="flex-none h-[200px] sm:h-[300px] retro-border bg-black/40 p-3 sm:p-5 relative overflow-hidden flex flex-col">
            <div className="flex items-center gap-2 sm:gap-3 text-green-500 border-b border-green-900/50 pb-2 mb-2 sm:mb-4 flex-none">
                <Command className="w-4 sm:w-5 h-4 sm:h-5" />
                <span className="font-bold tracking-widest text-[10px] sm:text-xs uppercase font-pixel">COMMAND TEMPLATES</span>
            </div>

            <div className="flex-grow overflow-y-auto custom-scrollbar">
                <div className="grid grid-cols-2 sm:grid-cols-2 lg:grid-cols-3 gap-2 sm:gap-3">
                    {TMUX_TEMPLATES.map((tpl) => (
                        <button
                            key={tpl.id}
                            onClick={() => applyTemplate(tpl.cmd)}
                            className="text-left group relative bg-black/60 border border-green-900/60 hover:border-green-400 p-2 sm:p-5 transition-all hover:bg-green-900/10 flex flex-col"
                        >
                            <div className="flex justify-between items-start mb-1 sm:mb-2">
                                <span className="text-green-300 font-bold tracking-wider group-hover:text-white transition-colors text-sm sm:text-lg">
                                    {tpl.label}
                                </span>
                            </div>
                            <div className="text-xs sm:text-sm text-green-600 font-mono truncate w-full opacity-70 mb-1 hidden sm:block">
                                {tpl.cmd}
                            </div>

                            {/* Corner accents */}
                            <div className="absolute top-0 right-0 w-2 h-2 border-t border-r border-transparent group-hover:border-green-400 transition-colors"></div>
                            <div className="absolute bottom-0 left-0 w-2 h-2 border-b border-l border-transparent group-hover:border-green-400 transition-colors"></div>
                        </button>
                    ))}
                </div>
            </div>
        </div>
        </>
        )}
    </div>
  );
};