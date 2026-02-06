import React, { useState, useRef, useEffect } from 'react';
import { ConsoleLog, ConsoleTarget } from '../types';
import { Command, ChevronRight, TerminalSquare } from 'lucide-react';

interface ConsoleViewProps {
    logs: ConsoleLog[];
    target: ConsoleTarget | null;
}

const TMUX_TEMPLATES = [
    { 
        id: 'send_keys', 
        label: 'Send Keys', 
        cmd: 'tmux send-keys -t {session}:{window} "{keys}" C-m',
        desc: 'Send text/keys to target'
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
  const [windowInput, setWindowInput] = useState('');
  const [paneInput, setPaneInput] = useState('3');
  const [keysInput, setKeysInput] = useState('');
  
  const inputRef = useRef<HTMLInputElement>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs]);

  useEffect(() => {
    if (target) {
        setSessionInput(target.session);
        setWindowInput(target.window);
        setLogs(prev => [...prev, {
            id: Date.now().toString(),
            type: 'system',
            text: `> Connected to target: ${target.session} :: ${target.window}`
        }]);
    }
  }, [target]);

  // Auto-focus input on mount
  useEffect(() => {
      inputRef.current?.focus();
  }, []);

  const handleSend = () => {
    if (!input.trim()) return;
    
    const newLog: ConsoleLog = {
        id: Date.now().toString(),
        type: 'input',
        text: `> ${input}`
    };
    
    setLogs(prev => [...prev, newLog]);
    setInput('');

    // Simulate response
    setTimeout(() => {
        setLogs(prev => [...prev, {
            id: (Date.now() + 1).toString(),
            type: 'output',
            text: `Command executed: ${input.split(' ')[0]}... OK`
        }]);
    }, 500);
  };

  const applyTemplate = (templateCmd: string) => {
      let cmd = templateCmd;
      
      // Auto-fill variables from inputs
      cmd = cmd.replace(/{session}/g, sessionInput || 'current_session');
      cmd = cmd.replace(/{window}/g, windowInput || 'current_window');
      cmd = cmd.replace(/{pane}/g, paneInput);
      
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
        {/* 1. Target Selectors Header */}
        <div className="flex flex-wrap items-center gap-4 text-green-500 font-mono uppercase p-4 border border-green-900/50 bg-black/40 shadow-[0_0_15px_rgba(0,0,0,0.5)_inset] flex-none">
            <span className="font-bold tracking-widest text-green-600 text-xl">CONNECTION:</span>
            
            <div className="flex items-center gap-2">
                <span className="text-green-800 text-sm">SESSION</span>
                <input
                    type="text"
                    value={sessionInput}
                    onChange={(e) => setSessionInput(e.target.value)}
                    placeholder="---"
                    className={`${inputClass} w-[160px]`}
                />
            </div>
            
            <div className="flex items-center gap-2">
                <span className="text-green-800 text-sm">WINDOW</span>
                <input
                    type="text"
                    value={windowInput}
                    onChange={(e) => setWindowInput(e.target.value)}
                    placeholder="---"
                    className={`${inputClass} w-[120px]`}
                />
            </div>

            <div className="flex items-center gap-2">
                <span className="text-green-800 text-sm">PANE</span>
                <input
                    type="text"
                    value={paneInput}
                    onChange={(e) => setPaneInput(e.target.value)}
                    placeholder="#"
                    className={`${inputClass} w-[70px] text-center`}
                />
            </div>
            
            {/* Keys Input */}
            <div className="flex items-center gap-2 flex-grow min-w-[200px]">
                <span className="text-green-800 text-sm">KEYS_VAR</span>
                <input
                    type="text"
                    value={keysInput}
                    onChange={(e) => setKeysInput(e.target.value)}
                    placeholder="Value for {keys}..."
                    className={`${inputClass} w-full`}
                />
            </div>
            
            <div className="ml-auto pl-4 border-l border-green-900/30">
                 <div className={`w-4 h-4 rounded-full ${sessionInput && windowInput ? 'bg-green-500 animate-pulse shadow-[0_0_8px_#22c55e]' : 'bg-red-900'}`}></div>
            </div>
        </div>

        {/* 2. Main Content: Terminal & Input (SWAPPED to TOP) */}
        <div 
            className="flex-grow flex flex-col min-h-0 retro-border bg-black/80 p-6 relative overflow-hidden shadow-[inset_0_0_50px_rgba(0,0,0,0.5)] cursor-text"
            onClick={() => inputRef.current?.focus()}
        >
             <div className="flex items-center gap-3 text-green-500/80 border-b border-green-900/50 pb-2 mb-2 flex-none">
                <TerminalSquare className="w-6 h-6" />
                <span className="font-bold tracking-widest text-lg uppercase">LIVE SESSION LOG</span>
            </div>

            {/* Scrollable Logs */}
            <div className="flex-grow overflow-y-auto custom-scrollbar font-mono text-xl space-y-1 p-2">
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
            <div className="flex-none flex items-center gap-3 relative border-t border-green-900/50 pt-4 mt-2">
                <span className="text-green-500 font-bold text-3xl animate-pulse select-none flex items-center h-full pb-1">{'>'}</span>
                
                <form onSubmit={(e) => { e.preventDefault(); handleSend(); }} className="flex-grow flex items-center">
                    <input
                        ref={inputRef}
                        type="text"
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        className="w-full bg-transparent border-none outline-none text-green-300 font-bold text-2xl p-0 focus:ring-0 placeholder-green-900/30 font-mono caret-green-500 h-10"
                        autoFocus
                        autoComplete="off"
                        placeholder="ENTER COMMAND..."
                    />
                </form>
                
                <div className="hidden md:flex items-center gap-2 px-3 py-1 bg-green-900/10 border border-green-900/30 rounded text-green-800 text-sm font-bold tracking-widest">
                    <span>EXECUTE</span>
                    <span className="border border-green-900 px-1 rounded bg-black/30">↵</span>
                </div>
            </div>
        </div>

        {/* 3. Bottom: Templates (SWAPPED to BOTTOM) */}
        <div className="flex-none h-[300px] retro-border bg-black/40 p-5 relative overflow-hidden flex flex-col">
            <div className="flex items-center gap-3 text-green-400 border-b border-green-800 pb-2 mb-4 flex-none">
                <Command className="w-6 h-6" />
                <span className="font-bold tracking-widest text-2xl retro-text-shadow">COMMAND TEMPLATES</span>
            </div>
            
            <div className="flex-grow overflow-y-auto custom-scrollbar">
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                    {TMUX_TEMPLATES.map((tpl) => (
                        <button
                            key={tpl.id}
                            onClick={() => applyTemplate(tpl.cmd)}
                            className="text-left group relative bg-black/60 border border-green-900/60 hover:border-green-400 p-5 transition-all hover:bg-green-900/10 flex flex-col"
                        >
                            <div className="flex justify-between items-start mb-2">
                                <span className="text-green-300 font-bold tracking-wider group-hover:text-white transition-colors text-lg">
                                    {tpl.label}
                                </span>
                            </div>
                            <div className="text-sm text-green-600 font-mono truncate w-full opacity-70 mb-1">
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
    </div>
  );
};