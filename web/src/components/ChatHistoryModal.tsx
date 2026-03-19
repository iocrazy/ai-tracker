import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { X, MessageSquare, Send, Paperclip, XCircle } from 'lucide-react';
import { tmuxSendKeys, tmuxSendRawKeys, sendImages, ToolInteraction, ToolCallInfo, ToolResultInfo, HookChatMessage } from '../services/api';
import { ClaudeStatus } from '../types';
import { ChatTimeline, fromLiveChatMessages } from './ChatTimeline';

export interface ChatMessage {
  sender: 'USER' | 'AGENT' | 'SYSTEM';
  text: string;
  timestamp: string;
  thinking?: string;
  interaction?: ToolInteraction;
  toolCalls?: ToolCallInfo[];
  toolResults?: ToolResultInfo[];
}

type SendStatus = 'idle' | 'sending' | 'success' | 'failed';

interface ChatHistoryModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  subtitle?: string;
  messages: ChatMessage[];
  hookMessages?: HookChatMessage[];  // Real-time hook messages to append
  sessionName?: string;
  windowName?: string;
  windowId?: string;  // tmux window ID (e.g., "@33") for send-keys targeting
  claudePane?: string;  // Pane number where Claude runs (default: "1")
  claudeStatus?: ClaudeStatus;  // Current Claude status for display
}

// Default pane where Claude runs (can be auto-detected or configured per window)
const DEFAULT_CLAUDE_PANE = '1';

export const ChatHistoryModal: React.FC<ChatHistoryModalProps> = ({ isOpen, onClose, title, subtitle, messages, hookMessages, sessionName, windowName, windowId, claudePane, claudeStatus }) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [inputValue, setInputValue] = useState('');
  const [isSending, setIsSending] = useState(false);
  // Detect if session is live (has active Claude) vs archived (no Claude running)
  const isLive = !subtitle?.includes('ARCHIVE');
  const [sendStatus, setSendStatus] = useState<SendStatus>('idle');
  const [isAtBottom, setIsAtBottom] = useState(true);  // Track if user is at bottom
  const [pendingImages, setPendingImages] = useState<{ file: File; preview: string }[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [sentInteractions, setSentInteractions] = useState<Set<number>>(new Set());

  // Merge hook messages into displayed messages
  const allMessages = useMemo(() => {
    if (!hookMessages || hookMessages.length === 0) return messages;
    const hookConverted: ChatMessage[] = hookMessages.map(m => ({
      sender: m.role === 'user' ? 'USER' : 'AGENT',
      text: m.content,
      timestamp: m.timestamp || '',
    }));
    return [...messages, ...hookConverted];
  }, [messages, hookMessages]);

  // Reset send status after a delay
  useEffect(() => {
    if (sendStatus === 'success' || sendStatus === 'failed') {
      const timer = setTimeout(() => setSendStatus('idle'), 3000);
      return () => clearTimeout(timer);
    }
  }, [sendStatus]);

  const fileToBase64 = useCallback((file: File): Promise<string> => {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result as string);
      reader.onerror = reject;
      reader.readAsDataURL(file);
    });
  }, []);

  const addImageFiles = useCallback((files: File[]) => {
    const imageFiles = files.filter(f => f.type.startsWith('image/'));
    if (imageFiles.length === 0) return;
    const newItems = imageFiles.map(file => ({
      file,
      preview: URL.createObjectURL(file),
    }));
    setPendingImages(prev => [...prev, ...newItems]);
  }, []);

  const removeImage = useCallback((index: number) => {
    setPendingImages(prev => {
      const removed = prev[index];
      if (removed) URL.revokeObjectURL(removed.preview);
      return prev.filter((_, i) => i !== index);
    });
  }, []);

  const clearAllImages = useCallback(() => {
    setPendingImages(prev => {
      prev.forEach(img => URL.revokeObjectURL(img.preview));
      return [];
    });
  }, []);

  const handlePaste = useCallback((e: ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    const files: File[] = [];
    for (const item of items) {
      if (item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) files.push(file);
      }
    }
    if (files.length > 0) {
      e.preventDefault();
      addImageFiles(files);
    }
  }, [addImageFiles]);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    const files = Array.from(e.dataTransfer.files).filter(f => f.type.startsWith('image/'));
    if (files.length > 0) addImageFiles(files);
  }, [addImageFiles]);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  }, []);

  const handleFileSelect = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files || []);
    if (files.length > 0) addImageFiles(files);
    e.target.value = '';
  }, [addImageFiles]);

  // Listen for paste events when modal is open
  useEffect(() => {
    if (!isOpen) return;
    document.addEventListener('paste', handlePaste);
    return () => document.removeEventListener('paste', handlePaste);
  }, [isOpen, handlePaste]);

  const [sendError, setSendError] = useState('');

  // Slash command palette
  const SLASH_COMMANDS = useMemo(() => [
    { command: '/brainstorming', description: 'Creative design and brainstorming' },
    { command: '/commit', description: 'Create a git commit' },
    { command: '/plan', description: 'Create implementation plan' },
    { command: '/review', description: 'Request code review' },
    { command: '/tdd', description: 'Test-driven development' },
    { command: '/debug', description: 'Systematic debugging' },
    { command: '/discord-notify', description: 'Send Discord notification' },
    { command: '/help', description: 'Show available commands' },
    { command: '/clear', description: 'Clear conversation' },
    { command: '/compact', description: 'Compact context' },
    { command: '/cost', description: 'Show session cost' },
    { command: '/doctor', description: 'Check Claude Code health' },
    { command: '/init', description: 'Initialize CLAUDE.md' },
    { command: '/login', description: 'Switch account' },
    { command: '/logout', description: 'Sign out' },
    { command: '/memory', description: 'Edit CLAUDE.md' },
    { command: '/model', description: 'Switch model' },
    { command: '/permissions', description: 'View permissions' },
    { command: '/pr', description: 'Create pull request' },
    { command: '/status', description: 'Show git status' },
    { command: '/terminal-setup', description: 'Terminal configuration' },
    { command: '/vim', description: 'Enter vim mode' },
  ], []);

  const [slashCommands, setSlashCommands] = useState<{ command: string; description: string }[]>([]);
  const [slashIndex, setSlashIndex] = useState(0);

  // Detect `/` prefix and filter commands
  useEffect(() => {
    const text = inputValue.trim();
    if (text.startsWith('/') && !text.includes(' ')) {
      const query = text.toLowerCase();
      const filtered = SLASH_COMMANDS.filter(c => c.command.toLowerCase().startsWith(query));
      setSlashCommands(filtered);
      setSlashIndex(0);
    } else {
      setSlashCommands([]);
    }
  }, [inputValue, SLASH_COMMANDS]);

  const handleSend = async () => {
    const hasText = inputValue.trim().length > 0;
    const hasImages = pendingImages.length > 0;
    if ((!hasText && !hasImages) || !sessionName || !windowId || isSending) return;

    const msgText = inputValue.trim();
    const savedInput = msgText; // Preserve for retry on failure
    setInputValue('');
    setIsSending(true);
    setSendStatus('sending');
    setSendError('');

    // Timeout wrapper (10s)
    const withTimeout = <T,>(promise: Promise<T>, ms = 10000): Promise<T> =>
      Promise.race([
        promise,
        new Promise<never>((_, reject) => setTimeout(() => reject(new Error('发送超时 (10s)')), ms)),
      ]);

    try {
      const targetPane = claudePane || DEFAULT_CLAUDE_PANE;

      if (hasImages) {
        const base64List = await Promise.all(pendingImages.map(img => fileToBase64(img.file)));
        const result = await withTimeout(sendImages(
          sessionName,
          windowId,
          targetPane,
          base64List,
          msgText || undefined
        ));
        if (result.success) {
          clearAllImages();
          setSendStatus('success');
        } else {
          setSendStatus('failed');
          setSendError(result.message || '发送失败');
          setInputValue(savedInput); // Restore input for retry
        }
      } else {
        const result = await withTimeout(tmuxSendKeys(sessionName, windowId, targetPane, msgText, 'Enter'));
        if (result.success) {
          setSendStatus('success');
        } else {
          setSendStatus('failed');
          setSendError(result.message || '发送失败');
          setInputValue(savedInput); // Restore input for retry
        }
      }
    } catch (error) {
      console.error('Failed to send message:', error);
      setSendStatus('failed');
      setSendError(error instanceof Error ? error.message : '发送失败');
      setInputValue(savedInput); // Restore input for retry
    } finally {
      setIsSending(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Check if user is at bottom of scroll area
  const checkIsAtBottom = () => {
    if (scrollRef.current) {
      const { scrollTop, scrollHeight, clientHeight } = scrollRef.current;
      // Consider "at bottom" if within 100px of the bottom
      const atBottom = scrollHeight - scrollTop - clientHeight < 100;
      setIsAtBottom(atBottom);
    }
  };

  // Auto-scroll to bottom when modal opens
  useEffect(() => {
    if (isOpen && scrollRef.current && allMessages.length > 0) {
      // Always scroll to bottom when modal first opens
      setTimeout(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
          setIsAtBottom(true);
        }
      }, 50);
    }
  }, [isOpen]);  // Only on open, not on messages change

  // Auto-scroll on new messages only if user is at bottom
  useEffect(() => {
    if (isOpen && scrollRef.current && allMessages.length > 0 && isAtBottom) {
      setTimeout(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
      }, 50);
    }
  }, [allMessages, isAtBottom, isOpen]);

  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't capture keys when typing in input
      if (document.activeElement === inputRef.current) {
        if (e.key === 'Escape') {
          inputRef.current?.blur();
        }
        return;
      }

      switch (e.key) {
        case 'Escape':
          onClose();
          break;
        case 'j': // Scroll Down
        case 'ArrowDown':
           if (scrollRef.current) scrollRef.current.scrollBy({ top: 50, behavior: 'smooth' });
           break;
        case 'k': // Scroll Up
        case 'ArrowUp':
           if (scrollRef.current) scrollRef.current.scrollBy({ top: -50, behavior: 'smooth' });
           break;
        case 'J': // Fast Scroll Down
           if (scrollRef.current) scrollRef.current.scrollBy({ top: 200, behavior: 'smooth' });
           break;
        case 'K': // Fast Scroll Up
           if (scrollRef.current) scrollRef.current.scrollBy({ top: -200, behavior: 'smooth' });
           break;
        case 'i': // Focus input (VIM insert mode)
           inputRef.current?.focus();
           e.preventDefault();
           break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out] overflow-y-auto">
      <div
        className="w-full max-w-3xl max-h-[90vh] flex flex-col retro-border bg-black shadow-[0_0_50px_rgba(34,197,94,0.3)] relative my-auto"
      >
        {/* Header */}
        <div className="flex items-center justify-between p-3 sm:p-6 border-b border-green-800 bg-green-900/20 flex-shrink-0">
            <div className="flex items-center gap-2 sm:gap-4 min-w-0">
                <MessageSquare className="w-5 sm:w-8 h-5 sm:h-8 text-green-400 flex-shrink-0" />
                <div className="min-w-0">
                    <h3 className="text-lg sm:text-3xl font-bold text-green-400 tracking-widest uppercase font-mono truncate">{title}</h3>
                    {subtitle && <p className="text-sm sm:text-lg text-green-600 font-mono tracking-wider mt-1 truncate">{subtitle}</p>}
                </div>
            </div>
            <button
                onClick={onClose}
                className="text-green-800 hover:text-green-400 transition-colors p-1 sm:p-2 flex-shrink-0"
                title="Close [ESC / h]"
            >
                <X className="w-6 sm:w-8 h-6 sm:h-8" />
            </button>
        </div>

        {/* Resume Session banner */}
        {claudeStatus?.awaiting_resume && (
          <div className="px-4 py-2 bg-yellow-900/30 border-b border-yellow-700/50 flex items-center justify-between">
            <span className="text-yellow-400 text-xs font-mono">
              ⏸ Claude 在等待选择 Resume Session — 消息无法发送
            </span>
            <button
              onClick={async () => {
                if (sessionName && windowId) {
                  const targetPane = claudePane || '1';
                  await tmuxSendKeys(sessionName, windowId, targetPane, '', 'Enter');
                }
              }}
              className="px-2 py-0.5 bg-yellow-800/50 border border-yellow-600/50 rounded text-yellow-300 text-xs hover:bg-yellow-700/50"
            >
              选择默认 Session
            </button>
          </div>
        )}

        {/* Content */}
        <div
          ref={scrollRef}
          onScroll={checkIsAtBottom}
          onDrop={handleDrop}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          className={`flex-grow overflow-y-auto overflow-x-hidden p-3 sm:p-4 space-y-2 sm:space-y-3 custom-scrollbar font-mono scroll-smooth relative ${isDragging ? 'ring-2 ring-green-400 ring-inset' : ''}`}
        >
            {/* Drag overlay */}
            {isDragging && (
              <div className="absolute inset-0 z-10 flex items-center justify-center bg-black/60 border-2 border-dashed border-green-400 pointer-events-none">
                <span className="text-green-400 text-lg font-mono tracking-wider">DROP IMAGE HERE</span>
              </div>
            )}
            <ChatTimeline
              items={fromLiveChatMessages(allMessages)}
              onInteractionSelect={isLive ? async (msgIdx, optIdx, multiSelect, totalOptions) => {
                if (!sessionName || !windowId) return;
                const targetPane = claudePane || DEFAULT_CLAUDE_PANE;

                try {
                  if (multiSelect) {
                    // Multi-select TUI: navigate with arrow keys + Space to toggle + Submit
                    // Claude Code's multi-select layout (cursor starts at first option):
                    //   pos 0..N-1: user options (with [ ] checkboxes)
                    //   pos N:      "Type something"
                    //   pos N+1:    Submit
                    //   pos N+2:    "Chat about this"
                    const keys: string[] = [];

                    if (optIdx === totalOptions + 1) {
                      // "Chat about this" — navigate to pos N+2 and Enter
                      for (let i = 0; i < totalOptions + 2; i++) keys.push('Down');
                      keys.push('Enter');
                    } else {
                      // Regular option: navigate to it, toggle, then navigate to Submit
                      for (let i = 0; i < optIdx; i++) keys.push('Down');
                      keys.push('Space');
                      // Navigate from current pos to Submit (pos N+1)
                      const stepsToSubmit = totalOptions + 1 - optIdx;
                      for (let i = 0; i < stepsToSubmit; i++) keys.push('Down');
                      keys.push('Enter');
                    }

                    await tmuxSendRawKeys(sessionName, windowId, targetPane, keys);
                  } else {
                    // Single-select: send option number + Enter
                    await tmuxSendKeys(sessionName, windowId, targetPane, String(optIdx + 1), 'Enter');
                  }

                  setSentInteractions(prev => new Set(prev).add(msgIdx));
                } catch (err) {
                  console.error('Failed to send interaction:', err);
                }
              } : undefined}
              onInteractionTextSubmit={isLive ? async (msgIdx, text, optionCount, multiSelect) => {
                if (!sessionName || !windowId) return;
                const targetPane = claudePane || DEFAULT_CLAUDE_PANE;

                try {
                  if (multiSelect) {
                    // Multi-select: navigate to "Type something" (pos N), toggle, go to Submit, Enter
                    const keys: string[] = [];
                    for (let i = 0; i < optionCount; i++) keys.push('Down');
                    keys.push('Space');  // Toggle "Type something"
                    keys.push('Down');   // Move to Submit
                    keys.push('Enter');  // Press Submit
                    await tmuxSendRawKeys(sessionName, windowId, targetPane, keys);
                    // Wait for Claude Code to show text input prompt
                    await new Promise(resolve => setTimeout(resolve, 500));
                    // Send the typed text
                    await tmuxSendKeys(sessionName, windowId, targetPane, text, 'Enter');
                  } else {
                    // Single-select: select "Other" option (options count + 1 in Claude Code's prompt)
                    await tmuxSendKeys(sessionName, windowId, targetPane, String(optionCount + 1), 'Enter');
                    // Wait for Claude Code to show text input prompt
                    await new Promise(resolve => setTimeout(resolve, 500));
                    // Send the typed text
                    await tmuxSendKeys(sessionName, windowId, targetPane, text, 'Enter');
                  }

                  setSentInteractions(prev => new Set(prev).add(msgIdx));
                } catch (err) {
                  console.error('Failed to send interaction text:', err);
                }
              } : undefined}
              sentInteractions={sentInteractions}
            />
        </div>

        {/* Input Area — only shown for live sessions */}
        {isLive && sessionName && windowId && (
          <div className="p-2 sm:p-3 border-t border-green-800 bg-green-900/10 flex-shrink-0">
            {/* Image previews */}
            {pendingImages.length > 0 && (
              <div className="mb-2 flex flex-wrap items-start gap-2 p-2 border border-green-800 bg-green-900/20">
                {pendingImages.map((img, idx) => (
                  <div key={img.preview} className="relative group">
                    <img
                      src={img.preview}
                      alt={`Preview ${idx + 1}`}
                      className="h-16 w-16 object-cover border border-green-800 rounded"
                    />
                    <button
                      onClick={() => removeImage(idx)}
                      className="absolute -top-1.5 -right-1.5 bg-black rounded-full text-green-700 hover:text-red-400 transition-colors opacity-0 group-hover:opacity-100"
                      title="移除"
                    >
                      <XCircle className="w-4 h-4" />
                    </button>
                  </div>
                ))}
                {pendingImages.length > 1 && (
                  <button
                    onClick={clearAllImages}
                    className="text-green-800 hover:text-red-400 text-xs font-mono self-center px-2 transition-colors"
                  >
                    清除全部
                  </button>
                )}
              </div>
            )}
            <div className="flex gap-2">
              {/* Paperclip button */}
              <button
                onClick={() => fileInputRef.current?.click()}
                disabled={isSending}
                className="px-2 py-2 text-green-700 hover:text-green-400 disabled:opacity-30 transition-colors flex-shrink-0"
                title="Attach image"
              >
                <Paperclip className="w-4 h-4" />
              </button>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                onChange={handleFileSelect}
                className="hidden"
              />
              <div className="flex-1 relative">
                <textarea
                  ref={inputRef}
                  value={inputValue}
                  onChange={(e) => {
                    setInputValue(e.target.value);
                    const el = e.target;
                    el.style.height = '36px';
                    el.style.height = Math.min(el.scrollHeight, 140) + 'px';
                  }}
                  onKeyDown={(e) => {
                    // Command palette navigation
                    if (slashCommands.length > 0) {
                      if (e.key === 'ArrowDown') {
                        e.preventDefault();
                        setSlashIndex(i => Math.min(i + 1, slashCommands.length - 1));
                        return;
                      }
                      if (e.key === 'ArrowUp') {
                        e.preventDefault();
                        setSlashIndex(i => Math.max(i - 1, 0));
                        return;
                      }
                      if (e.key === 'Tab' || e.key === 'Enter') {
                        if (slashCommands[slashIndex]) {
                          e.preventDefault();
                          setInputValue(slashCommands[slashIndex].command + ' ');
                          setSlashCommands([]);
                          return;
                        }
                      }
                      if (e.key === 'Escape') {
                        setSlashCommands([]);
                        return;
                      }
                    }
                    handleKeyDown(e);
                  }}
                  placeholder="Type a message..."
                  disabled={isSending}
                  rows={1}
                  className="w-full bg-black border border-green-800 text-green-400 px-3 py-2 text-sm font-mono placeholder-green-900 focus:outline-none focus:border-green-500 disabled:opacity-50 resize-none"
                  style={{ minHeight: '36px', maxHeight: '140px', lineHeight: '1.5' }}
                />
                {/* Command palette dropdown */}
                {slashCommands.length > 0 && (
                  <div className="absolute bottom-full left-0 right-0 mb-1 bg-black border border-green-800 max-h-48 overflow-y-auto z-50 font-mono text-xs">
                    {slashCommands.map((cmd, i) => (
                      <div
                        key={cmd.command}
                        className={`px-3 py-1.5 cursor-pointer flex items-center gap-3 ${i === slashIndex ? 'bg-green-900/50 text-green-300' : 'text-green-600 hover:bg-green-900/20'}`}
                        onClick={() => {
                          setInputValue(cmd.command + ' ');
                          setSlashCommands([]);
                          inputRef.current?.focus();
                        }}
                      >
                        <span className="text-cyan-500 min-w-[140px]">{cmd.command}</span>
                        <span className="text-green-800 truncate">{cmd.description}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
              <button
                onClick={handleSend}
                disabled={(!inputValue.trim() && pendingImages.length === 0) || isSending}
                className="px-4 py-2 bg-green-900/50 border border-green-700 text-green-400 hover:bg-green-800 hover:text-green-300 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              >
                <Send className="w-4 h-4" />
              </button>
            </div>
          </div>
        )}

        {/* Footer with status */}
        <div className="p-2 sm:p-3 border-t border-green-900/50 bg-black px-3 sm:px-6 flex-shrink-0">
            {/* Status line */}
            <div className="flex items-center justify-between text-[10px] sm:text-xs font-mono mb-1">
                {/* Send status */}
                <div className="flex items-center gap-2">
                    {sendStatus === 'sending' && (
                        <span className="text-yellow-500 animate-pulse">⏳ SENDING...</span>
                    )}
                    {sendStatus === 'success' && (
                        <span className="text-green-500">🟢 SENT</span>
                    )}
                    {sendStatus === 'failed' && (
                        <span className="text-red-500" title={sendError}>🔴 {sendError || 'FAILED'}</span>
                    )}
                </div>
                {/* Claude status */}
                {claudeStatus && (claudeStatus.current_tool || claudeStatus.action) && (
                    <div className="text-yellow-500 truncate max-w-[60%] flex items-center gap-1" title={claudeStatus.current_tool || claudeStatus.action || ''}>
                        <span className="text-yellow-600">●</span>
                        <span className="truncate">{claudeStatus.current_tool || claudeStatus.action}</span>
                    </div>
                )}
            </div>
            {/* Help line */}
            <div className="flex justify-between items-center">
                <span className="text-[10px] sm:text-xs text-green-800 uppercase tracking-widest">SCROLL: [J/K] • IMG: [⌘V/📎] • CLOSE: [ESC]</span>
                <span className="text-[10px] sm:text-xs text-green-800 uppercase tracking-wider">LIVE_CHAT</span>
            </div>
        </div>
      </div>
    </div>
  );
};
