import React, { useEffect, useRef, useState } from 'react';
import { X, MessageSquare, Send } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import { tmuxSendKeys } from '../services/api';
import { ClaudeStatus } from '../types';

export interface ChatMessage {
  sender: 'USER' | 'AGENT' | 'SYSTEM';
  text: string;
  timestamp: string;
}

type SendStatus = 'idle' | 'sending' | 'success' | 'failed';

interface ChatHistoryModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  subtitle?: string;
  messages: ChatMessage[];
  sessionName?: string;
  windowName?: string;
  windowId?: string;  // tmux window ID (e.g., "@33") for send-keys targeting
  claudePane?: string;  // Pane number where Claude runs (default: "1")
  claudeStatus?: ClaudeStatus;  // Current Claude status for display
}

// Default pane where Claude runs (can be auto-detected or configured per window)
const DEFAULT_CLAUDE_PANE = '1';

export const ChatHistoryModal: React.FC<ChatHistoryModalProps> = ({ isOpen, onClose, title, subtitle, messages, sessionName, windowName, windowId, claudePane, claudeStatus }) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [inputValue, setInputValue] = useState('');
  const [isSending, setIsSending] = useState(false);
  const [sendStatus, setSendStatus] = useState<SendStatus>('idle');
  const [isAtBottom, setIsAtBottom] = useState(true);  // Track if user is at bottom

  // Reset send status after a delay
  useEffect(() => {
    if (sendStatus === 'success' || sendStatus === 'failed') {
      const timer = setTimeout(() => setSendStatus('idle'), 3000);
      return () => clearTimeout(timer);
    }
  }, [sendStatus]);

  const handleSend = async () => {
    if (!inputValue.trim() || !sessionName || !windowId || isSending) return;

    const msgText = inputValue.trim();
    setInputValue('');
    setIsSending(true);
    setSendStatus('sending');

    try {
      // Send to tmux window at configured pane (where Claude runs)
      const targetPane = claudePane || DEFAULT_CLAUDE_PANE;
      const result = await tmuxSendKeys(sessionName, windowId, targetPane, msgText, 'Enter');
      setSendStatus(result.success ? 'success' : 'failed');
    } catch (error) {
      console.error('Failed to send message:', error);
      setSendStatus('failed');
    } finally {
      setIsSending(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
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
    if (isOpen && scrollRef.current && messages.length > 0) {
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
    if (isOpen && scrollRef.current && messages.length > 0 && isAtBottom) {
      setTimeout(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
      }, 50);
    }
  }, [messages, isAtBottom, isOpen]);

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

        {/* Content */}
        <div ref={scrollRef} onScroll={checkIsAtBottom} className="flex-grow overflow-y-auto overflow-x-hidden p-3 sm:p-4 space-y-2 sm:space-y-3 custom-scrollbar font-mono scroll-smooth">
            {messages.length === 0 ? (
                <div className="text-center text-green-900 py-8 italic text-base sm:text-lg">NO_DATA_FOUND_IN_ARCHIVE</div>
            ) : (
                messages.map((msg, idx) => (
                    <div key={idx} className={`flex gap-2 ${msg.sender === 'USER' ? 'flex-row-reverse' : ''}`}>
                        <div className={`
                            max-w-[90%] sm:max-w-[85%] p-2 sm:p-3 border leading-snug text-xs sm:text-sm
                            ${msg.sender === 'SYSTEM'
                                ? 'w-full text-center border-none text-green-700 italic text-xs sm:text-sm'
                                : msg.sender === 'USER'
                                    ? 'border-green-600/50 bg-green-900/10 text-green-300 rounded-tl-lg rounded-br-lg rounded-bl-lg'
                                    : 'border-green-800/50 text-green-400 rounded-tr-lg rounded-br-lg rounded-bl-lg'
                            }
                        `}>
                            {msg.sender !== 'SYSTEM' && (
                                <div className={`text-[10px] sm:text-xs font-bold mb-1 opacity-70 ${msg.sender === 'USER' ? 'text-right' : 'text-left'}`}>
                                    {msg.sender} <span className="font-normal mx-1">|</span> {msg.timestamp}
                                </div>
                            )}
                            <div className="prose prose-invert prose-green prose-xs sm:prose-sm max-w-none break-words
                                prose-p:my-0.5 prose-p:leading-snug prose-headings:text-green-400 prose-headings:my-1
                                prose-code:text-green-300 prose-code:bg-green-900/30 prose-code:px-1 prose-code:rounded prose-code:text-xs prose-code:break-all
                                prose-pre:bg-green-900/20 prose-pre:border prose-pre:border-green-800 prose-pre:my-1 prose-pre:p-2 prose-pre:overflow-x-auto prose-pre:max-w-full
                                prose-strong:text-green-300 prose-em:text-green-400
                                prose-ul:my-0.5 prose-ol:my-0.5 prose-li:my-0 prose-li:leading-snug">
                                <ReactMarkdown>{msg.text}</ReactMarkdown>
                            </div>
                        </div>
                    </div>
                ))
            )}
        </div>

        {/* Input Area */}
        {sessionName && windowId && (
          <div className="p-2 sm:p-3 border-t border-green-800 bg-green-900/10 flex-shrink-0">
            <div className="flex gap-2">
              <input
                ref={inputRef}
                type="text"
                value={inputValue}
                onChange={(e) => setInputValue(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Type a message..."
                disabled={isSending}
                className="flex-1 bg-black border border-green-800 text-green-400 px-3 py-2 text-sm font-mono placeholder-green-900 focus:outline-none focus:border-green-500 disabled:opacity-50"
              />
              <button
                onClick={handleSend}
                disabled={!inputValue.trim() || isSending}
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
                        <span className="text-red-500">🔴 FAILED</span>
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
                <span className="text-[10px] sm:text-xs text-green-800 uppercase tracking-widest">SCROLL: [J/K] • CLOSE: [ESC]</span>
                <span className="text-[10px] sm:text-xs text-green-800 uppercase tracking-wider">LIVE_CHAT</span>
            </div>
        </div>
      </div>
    </div>
  );
};