import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { X, MessageSquare, Send, Paperclip, XCircle } from 'lucide-react';
import { tmuxSendKeys, tmuxSendRawKeys, sendImages, fetchClaudeStatus, ToolInteraction, ToolCallInfo, ToolResultInfo, HookChatMessage } from '../services/api';
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
  isLive?: boolean;  // Explicit live/archive flag (default: true if not specified)
  messages: ChatMessage[];
  hookMessages?: HookChatMessage[];  // Real-time hook messages to append
  sessionName?: string;
  windowName?: string;
  windowId?: string;  // tmux window ID (e.g., "@33") for send-keys targeting
  claudePane?: string;  // Pane number where Claude runs (default: "1")
  claudeStatus?: ClaudeStatus;  // Current Claude status for display
  draftsRef?: React.RefObject<Map<string, string>>;  // Per-window draft storage from parent
}

// Default pane where Claude runs (can be auto-detected or configured per window)
const DEFAULT_CLAUDE_PANE = '1'; // Fallback if claudePane not detected

export const ChatHistoryModal: React.FC<ChatHistoryModalProps> = ({ isOpen, onClose, title, subtitle, isLive: isLiveProp, messages, hookMessages, sessionName, windowName, windowId, claudePane, claudeStatus, draftsRef }) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [inputValue, setInputValue] = useState('');
  const [isSending, setIsSending] = useState(false);
  const [uploadPercent, setUploadPercent] = useState<number | null>(null);
  // Use explicit prop if provided, fallback to subtitle heuristic for backwards compatibility
  const isLive = isLiveProp ?? !subtitle?.includes('ARCHIVE');
  const [sendStatus, setSendStatus] = useState<SendStatus>('idle');
  const [isAtBottom, setIsAtBottom] = useState(true);  // Track if user is at bottom
  const [pendingImages, setPendingImages] = useState<{ file: File; preview: string }[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [sentInteractions, setSentInteractions] = useState<Set<number>>(new Set());
  // Pending menu preview: local override from hover navigation
  const [menuPreview, setMenuPreview] = useState<string | null>(null);
  const [menuSelectedIdx, setMenuSelectedIdx] = useState<number | null>(null);
  const [menuExpandedOpt, setMenuExpandedOpt] = useState<number | null>(null);
  const menuHoverTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const menuNavigating = useRef(false);
  const prevWindowIdRef = useRef<string | undefined>(undefined);
  // Dynamically resolved Claude pane — updated on open + periodically
  const resolvedPaneRef = useRef<string>(claudePane || DEFAULT_CLAUDE_PANE);

  // Save draft on every input change
  useEffect(() => {
    if (windowId && draftsRef?.current) {
      draftsRef.current.set(windowId, inputValue);
    }
  }, [inputValue, windowId, draftsRef]);

  // Restore draft when opening a different window
  useEffect(() => {
    if (windowId && windowId !== prevWindowIdRef.current) {
      setInputValue(draftsRef?.current?.get(windowId) || '');
      setSendStatus('idle');
      setSentInteractions(new Set());
      // Reset pane target immediately: %pane IDs are global across tmux, so a
      // stale ref from the previous window would send keys into the wrong pane
      resolvedPaneRef.current = claudePane || DEFAULT_CLAUDE_PANE;
    }
    prevWindowIdRef.current = windowId;
  }, [windowId, draftsRef, claudePane]);

  // Clear interaction state on modal open; revoke image URLs on close
  useEffect(() => {
    if (isOpen) {
      setSentInteractions(new Set());
      setSendStatus('idle');
    } else {
      // Revoke any pending image object URLs to prevent memory leaks
      setPendingImages(prev => {
        prev.forEach(img => URL.revokeObjectURL(img.preview));
        return [];
      });
    }
  }, [isOpen]);

  // Resolve Claude pane from API
  const resolveClaudePane = useCallback(async (): Promise<string> => {
    if (!sessionName || !windowName) return claudePane || DEFAULT_CLAUDE_PANE;
    try {
      const res = await fetch(`/api/tmux/claude-status?session=${encodeURIComponent(sessionName)}&window=${encodeURIComponent(windowName)}`, {
        headers: { 'Authorization': `Bearer ${localStorage.getItem('agent-tracker-auth-token') || ''}` },
      });
      if (res.ok) {
        const data = await res.json();
        if (data.success && data.status?.pane) {
          resolvedPaneRef.current = data.status.pane;
          return data.status.pane;
        }
      }
    } catch { /* fallback */ }
    return resolvedPaneRef.current;
  }, [sessionName, windowName, claudePane]);

  // Resolve pane on modal open and refresh every 30s
  useEffect(() => {
    if (!isOpen || !isLive) return;
    resolveClaudePane();
    const interval = setInterval(resolveClaudePane, 30000);
    return () => clearInterval(interval);
  }, [isOpen, isLive, resolveClaudePane]);

  // Update ref when prop changes
  useEffect(() => {
    if (claudePane) resolvedPaneRef.current = claudePane;
  }, [claudePane]);

  // Merge hook messages into displayed messages (deduplicated)
  // Transcript (messages) refreshes every 3s and is authoritative.
  // Hook messages provide instant feedback for user prompts before transcript catches up.
  // Only append hook USER messages not already in transcript; skip AGENT (transcript has full version).
  const allMessages = useMemo(() => {
    if (!hookMessages || hookMessages.length === 0) return messages;
    // Normalize text for comparison: strip whitespace, take first 60 chars
    const normalize = (s: string) => s.replace(/\s+/g, ' ').trim().slice(0, 60);
    const existingKeys = new Set(
      messages.map(m => `${m.sender}:${normalize(m.text || '')}`)
    );
    // Determine the oldest transcript message timestamp to filter out stale hook messages
    // from previous Claude sessions that no longer exist in the current JSONL file
    const oldestTranscriptTs = messages.length > 0
      ? messages.reduce((oldest, m) => {
          if (!m.timestamp) return oldest;
          return !oldest || m.timestamp < oldest ? m.timestamp : oldest;
        }, '' as string)
      : '';
    const hookConverted: ChatMessage[] = hookMessages
      .filter(m => m.role === 'user') // Only user messages — agent messages come from transcript
      .filter(m => {
        // Skip hook messages older than the oldest transcript message
        // This prevents stale messages from a previous Claude session leaking in
        if (oldestTranscriptTs && m.timestamp && m.timestamp < oldestTranscriptTs) return false;
        return true;
      })
      .map(m => ({
        sender: 'USER' as ChatMessage['sender'],
        text: m.content,
        timestamp: m.timestamp || '',
      }))
      .filter(m => !existingKeys.has(`${m.sender}:${normalize(m.text || '')}`));
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

  const ACCEPTED_TYPES = ['image/', 'application/pdf', 'application/msword', 'application/vnd.openxmlformats-officedocument.wordprocessingml.document', 'text/markdown'];
  const ACCEPTED_EXTENSIONS = ['.md', '.markdown'];
  const isAcceptedFile = useCallback((f: File) =>
    ACCEPTED_TYPES.some(t => t.endsWith('/') ? f.type.startsWith(t) : f.type === t)
    || (f.name && ACCEPTED_EXTENSIONS.some(ext => f.name.toLowerCase().endsWith(ext)))
  , []);

  const addImageFiles = useCallback((files: File[]) => {
    const accepted = files.filter(isAcceptedFile);
    if (accepted.length === 0) return;
    const newItems = accepted.map(file => ({
      file,
      preview: file.type.startsWith('image/') ? URL.createObjectURL(file) : '',
    }));
    setPendingImages(prev => [...prev, ...newItems]);
  }, [isAcceptedFile]);

  const removeImage = useCallback((index: number) => {
    setPendingImages(prev => {
      const removed = prev[index];
      if (removed?.preview) URL.revokeObjectURL(removed.preview);
      return prev.filter((_, i) => i !== index);
    });
  }, []);

  const clearAllImages = useCallback(() => {
    setPendingImages(prev => {
      prev.forEach(img => { if (img.preview) URL.revokeObjectURL(img.preview); });
      return [];
    });
  }, []);

  const handlePaste = useCallback((e: ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    const files: File[] = [];
    for (const item of items) {
      if (isAcceptedFile({ type: item.type } as File)) {
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
    const files = Array.from(e.dataTransfer.files).filter(isAcceptedFile);
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

  // Prevent browser default file-open on drag/drop anywhere when modal is open
  useEffect(() => {
    if (!isOpen) return;
    const blockDrag = (e: DragEvent) => { e.preventDefault(); e.stopPropagation(); };
    document.addEventListener('dragover', blockDrag);
    document.addEventListener('drop', blockDrag);
    return () => {
      document.removeEventListener('dragover', blockDrag);
      document.removeEventListener('drop', blockDrag);
    };
  }, [isOpen]);

  const [sendError, setSendError] = useState('');

  // Slash command palette
  const SLASH_COMMANDS = useMemo(() => [
    // gstack — Planning
    { command: '/office-hours', description: 'YC office hours: reframe requirements' },
    { command: '/plan-ceo-review', description: 'CEO review: 10-dimension scope check' },
    { command: '/plan-eng-review', description: 'Eng review: architecture & test matrix' },
    { command: '/plan-design-review', description: 'Design review: UI/UX gaps' },
    { command: '/autoplan', description: 'Auto-run full planning pipeline' },
    // gstack — Development
    { command: '/design-consultation', description: 'Design consultation' },
    { command: '/review', description: 'Code review + auto-fix issues' },
    { command: '/investigate', description: 'Deep bug investigation' },
    // gstack — Quality
    { command: '/qa', description: 'QA test with real browser + auto-fix' },
    { command: '/qa-only', description: 'QA report only, no fixes' },
    { command: '/benchmark', description: 'Performance benchmark' },
    { command: '/design-review', description: 'UI/UX design review' },
    // gstack — Security
    { command: '/cso', description: 'Security audit: OWASP + STRIDE' },
    // gstack — Release
    { command: '/ship', description: 'Create PR' },
    { command: '/land-and-deploy', description: 'Merge and deploy' },
    { command: '/canary', description: 'Post-deploy monitoring' },
    { command: '/document-release', description: 'Release documentation' },
    // gstack — Tools
    { command: '/browse', description: 'Headless Chromium browser' },
    { command: '/retro', description: 'Analysis retrospective' },
    { command: '/codex', description: 'Cross-model review (OpenAI)' },
    { command: '/careful', description: 'Destructive command warnings' },
    { command: '/freeze', description: 'Lock files from editing' },
    { command: '/guard', description: 'Full safety mode (careful + freeze)' },
    { command: '/gstack-upgrade', description: 'Update gstack' },
    { command: '/gstack', description: 'gstack browser toolkit' },
    // Superpowers
    { command: '/brainstorming', description: 'Creative design and brainstorming' },
    { command: '/commit', description: 'Create a git commit' },
    { command: '/plan', description: 'Create implementation plan' },
    { command: '/tdd', description: 'Test-driven development' },
    { command: '/debug', description: 'Systematic debugging' },
    { command: '/simplify', description: 'Review code for quality and efficiency' },
    // Custom
    { command: '/discord-notify', description: 'Send Discord notification' },
    // Built-in commands
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
    if ((!hasText && !hasImages) || isSending) return;
    if (!sessionName || !windowId) {
      setSendStatus('failed');
      setSendError('无法发送: 未关联活跃窗口');
      return;
    }
    if (claudeStatus?.awaiting_resume) {
      setSendStatus('failed');
      setSendError('无法发送: Claude 等待选择 Session');
      return;
    }

    const msgText = inputValue.trim();
    setInputValue('');
    setIsSending(true);
    setSendStatus('sending');
    setSendError('');

    // Only restore input on failure if user hasn't typed anything new
    const restoreIfEmpty = (saved: string) => {
      setInputValue(prev => prev === '' ? saved : prev);
    };

    // Timeout wrapper (text: 10s, images: 30s)
    const withTimeout = <T,>(promise: Promise<T>, ms: number): Promise<T> =>
      Promise.race([
        promise,
        new Promise<never>((_, reject) => setTimeout(() => reject(new Error(`发送超时 (${ms / 1000}s)`)), ms)),
      ]);

    try {
      // Resolve Claude pane dynamically (refresh the cached value)
      const targetPane = await resolveClaudePane();

      if (hasImages) {
        const base64List = await Promise.all(pendingImages.map(img => fileToBase64(img.file)));
        setUploadPercent(0);
        const result = await withTimeout(sendImages(
          sessionName,
          windowId,
          targetPane,
          base64List,
          msgText || undefined,
          (pct) => setUploadPercent(pct)
        ), 30000);
        if (result.success) {
          clearAllImages();
          setSendStatus('success');
        } else {
          setSendStatus('failed');
          setSendError(result.message || '发送失败');
          restoreIfEmpty(msgText); // Restore input for retry (only if user hasn't typed new text)
        }
      } else {
        const result = await withTimeout(tmuxSendKeys(sessionName, windowId, targetPane, msgText, 'Enter'), 30000);
        if (result.success) {
          setSendStatus('success');
        } else {
          setSendStatus('failed');
          setSendError(result.message || '发送失败');
          restoreIfEmpty(msgText); // Restore input for retry (only if user hasn't typed new text)
        }
      }
    } catch (error) {
      console.error('Failed to send message:', error);
      setSendStatus('failed');
      setSendError(error instanceof Error ? error.message : '发送失败');
      restoreIfEmpty(msgText); // Restore input for retry (only if user hasn't typed new text)
    } finally {
      setIsSending(false);
      setUploadPercent(null);
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
    <div
      onDrop={handleDrop}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out] overflow-y-auto"
    >
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
          <div className="px-4 py-2 bg-yellow-900/30 border-b border-yellow-700/50 flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <span className="text-yellow-400 text-xs font-mono">
                ⏸ Claude 在等待选择 Resume Session — 消息无法发送
              </span>
              <div className="flex gap-2">
                <button
                  onClick={async () => {
                    if (sessionName && windowId) {
                      // Clear search box (Ctrl+U) then Enter to select first item
                      const pane = resolvedPaneRef.current;
                      await tmuxSendRawKeys(sessionName, windowId, pane, ['C-u', 'Enter']);
                    }
                  }}
                  className="px-2 py-0.5 bg-yellow-800/50 border border-yellow-600/50 rounded text-yellow-300 text-xs hover:bg-yellow-700/50"
                >
                  Resume 最近
                </button>
                <button
                  onClick={async () => {
                    if (sessionName && windowId) {
                      // Esc to cancel resume → starts new session
                      await tmuxSendKeys(sessionName, windowId, resolvedPaneRef.current, '', 'Escape');
                    }
                  }}
                  className="px-2 py-0.5 bg-green-900/50 border border-green-700/50 rounded text-green-400 text-xs hover:bg-green-800/50"
                >
                  新建 Session
                </button>
              </div>
            </div>
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
                <span className="text-green-400 text-lg font-mono tracking-wider">DROP FILE HERE</span>
              </div>
            )}
            <ChatTimeline
              items={fromLiveChatMessages(
                claudeStatus?.pending_menu
                  ? allMessages.map(m => m.interaction ? { ...m, interaction: undefined } : m)
                  : allMessages
              )}
              onInteractionSelect={isLive ? async (msgIdx, optIdx, multiSelect, totalOptions) => {
                if (!sessionName || !windowId) return;
                const targetPane = resolvedPaneRef.current;

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
                const targetPane = resolvedPaneRef.current;

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

            {/* Inline TUI menu from pane capture.
                Always show when present — matches live tmux display exactly.
                JSONL interaction is hidden when pending_menu exists (see ChatTimeline filter below). */}
            {claudeStatus?.pending_menu && (() => {
              const menu = claudeStatus.pending_menu!;
              const currentPreview = menuPreview ?? menu.preview ?? null;
              const effectiveSelected = menuSelectedIdx ?? menu.options.findIndex(o => o.selected);

              // Navigate cursor to an option (hover preview)
              const navigateToOption = (optIndex: number) => {
                if (!sessionName || !windowId || menuNavigating.current) return;
                const currentSelected = menu.options.findIndex(o => o.selected);
                const targetPos = menu.options.findIndex(o => o.index === optIndex);
                if (currentSelected < 0 || targetPos < 0 || targetPos === currentSelected) return;
                if (menuHoverTimer.current) clearTimeout(menuHoverTimer.current);
                menuHoverTimer.current = setTimeout(async () => {
                  menuNavigating.current = true;
                  const pane = resolvedPaneRef.current;
                  const delta = targetPos - currentSelected;
                  const keys: string[] = [];
                  const dir = delta > 0 ? 'Down' : 'Up';
                  for (let k = 0; k < Math.abs(delta); k++) keys.push(dir);
                  try {
                    await tmuxSendRawKeys(sessionName!, windowId!, pane, keys);
                    await new Promise(r => setTimeout(r, 200));
                    const resp = await fetchClaudeStatus(sessionName!, windowName || '');
                    if (resp.status?.pending_menu) {
                      setMenuPreview(resp.status.pending_menu.preview ?? null);
                      setMenuSelectedIdx(resp.status.pending_menu.options.findIndex((o: { selected: boolean }) => o.selected));
                    }
                  } catch (err) {
                    console.error('Menu navigation failed:', err);
                  } finally {
                    menuNavigating.current = false;
                  }
                }, 300);
              };

              // Single-select: navigate + Enter
              const selectOption = async (optIndex: number) => {
                if (!sessionName || !windowId) return;
                const opt = menu.options.find(o => o.index === optIndex);
                if (!confirm(`确认选择「${opt?.label || optIndex}」？`)) return;
                if (menuHoverTimer.current) clearTimeout(menuHoverTimer.current);
                const pane = resolvedPaneRef.current;
                const currentSelected = menu.options.findIndex(o => o.selected);
                const targetPos = menu.options.findIndex(o => o.index === optIndex);
                const keys: string[] = [];
                if (currentSelected >= 0 && targetPos >= 0 && targetPos !== currentSelected) {
                  const delta = targetPos - currentSelected;
                  const dir = delta > 0 ? 'Down' : 'Up';
                  for (let k = 0; k < Math.abs(delta); k++) keys.push(dir);
                }
                keys.push('Enter');
                await tmuxSendRawKeys(sessionName, windowId, pane, keys);
                setMenuPreview(null);
                setMenuSelectedIdx(null);
              };

              // Multi-select: navigate + Space to toggle checkbox, then re-fetch state
              const toggleOption = async (optIndex: number) => {
                if (!sessionName || !windowId || menuNavigating.current) return;
                menuNavigating.current = true;
                const pane = resolvedPaneRef.current;
                const currentSelected = menu.options.findIndex(o => o.selected);
                const targetPos = menu.options.findIndex(o => o.index === optIndex);
                const keys: string[] = [];
                if (currentSelected >= 0 && targetPos >= 0 && targetPos !== currentSelected) {
                  const delta = targetPos - currentSelected;
                  const dir = delta > 0 ? 'Down' : 'Up';
                  for (let k = 0; k < Math.abs(delta); k++) keys.push(dir);
                }
                keys.push('Space'); // Toggle checkbox
                try {
                  await tmuxSendRawKeys(sessionName, windowId, pane, keys);
                  await new Promise(r => setTimeout(r, 200));
                  // Re-fetch to get updated checkbox state
                  const resp = await fetchClaudeStatus(sessionName, windowName || '');
                  if (resp.status?.pending_menu) {
                    setMenuPreview(resp.status.pending_menu.preview ?? null);
                    setMenuSelectedIdx(resp.status.pending_menu.options.findIndex((o: { selected: boolean }) => o.selected));
                  }
                } catch (err) {
                  console.error('Toggle failed:', err);
                } finally {
                  menuNavigating.current = false;
                }
              };

              // Multi-select: submit (navigate to Submit line + Enter)
              const submitMultiSelect = async () => {
                if (!sessionName || !windowId) return;
                const checkedLabels = menu.options.filter(o => o.checked).map(o => o.label).join(', ');
                if (!confirm(`确认提交选择？\n已选: ${checkedLabels || '无'}`)) return;
                const pane = resolvedPaneRef.current;
                // Navigate from current position to Submit: go past all options + "Type something"
                const currentSelected = menu.options.findIndex(o => o.selected);
                const stepsToSubmit = menu.options.length - currentSelected; // options left + "Type something" = Submit
                const keys: string[] = [];
                for (let k = 0; k < stepsToSubmit; k++) keys.push('Down');
                keys.push('Enter');
                await tmuxSendRawKeys(sessionName, windowId, pane, keys);
                setMenuPreview(null);
                setMenuSelectedIdx(null);
              };

              return (
                <div className="mt-3 border border-green-700/50 bg-green-900/10 p-3">
                  {menu.header && (
                    <div className="text-[10px] sm:text-xs text-green-600 font-mono mb-1 uppercase tracking-wider">{menu.header}</div>
                  )}
                  {menu.question && (
                    <div className="text-xs sm:text-sm text-green-400 font-mono mb-2">{menu.question}</div>
                  )}
                  {menu.multi_select && (
                    <div className="text-[10px] text-green-700 font-mono mb-1">[MULTI-SELECT] — 点击切换勾选，完成后点 Submit</div>
                  )}
                  <div className="flex flex-col sm:flex-row gap-3">
                    {/* Left: options */}
                    <div className="flex flex-col gap-1.5 sm:min-w-[45%] sm:max-w-[55%]">
                      {menu.options.map((opt, idx) => {
                        const isExpanded = menuExpandedOpt === opt.index;
                        const hasDesc = !!opt.description;
                        const isTypeInput = opt.label === 'Type something' || opt.label === 'Type something.';
                        const isChatAbout = opt.label === 'Chat about this';
                        const isHighlighted = effectiveSelected >= 0 ? idx === effectiveSelected : opt.selected;

                        // "Type something" → render as input box
                        if (isTypeInput) {
                          return (
                            <div key={opt.index} className="flex gap-1.5">
                              <input
                                type="text"
                                placeholder="输入自定义回复..."
                                className="flex-1 bg-black border border-green-700/50 text-green-400 px-2 py-1.5 text-xs font-mono placeholder:text-green-900 focus:border-green-500 focus:outline-none"
                                onKeyDown={async (e) => {
                                  if (e.key === 'Enter') {
                                    const text = (e.target as HTMLInputElement).value.trim();
                                    if (!text || !sessionName || !windowId) return;
                                    const pane = resolvedPaneRef.current;
                                    // Navigate to "Type something", Enter to select it, then type text + Enter
                                    const currentSelected = menu.options.findIndex(o => o.selected);
                                    const targetPos = idx;
                                    const keys: string[] = [];
                                    if (currentSelected >= 0 && targetPos !== currentSelected) {
                                      const delta = targetPos - currentSelected;
                                      const dir = delta > 0 ? 'Down' : 'Up';
                                      for (let k = 0; k < Math.abs(delta); k++) keys.push(dir);
                                    }
                                    keys.push('Enter');
                                    await tmuxSendRawKeys(sessionName, windowId, pane, keys);
                                    await new Promise(r => setTimeout(r, 500));
                                    await tmuxSendKeys(sessionName, windowId, pane, text, 'Enter');
                                  }
                                }}
                              />
                              <button
                                onClick={async () => {
                                  const input = document.querySelector('input[placeholder="输入自定义回复..."]') as HTMLInputElement;
                                  const text = input?.value.trim();
                                  if (!text || !sessionName || !windowId) return;
                                  const pane = resolvedPaneRef.current;
                                  const currentSelected = menu.options.findIndex(o => o.selected);
                                  const targetPos = idx;
                                  const keys: string[] = [];
                                  if (currentSelected >= 0 && targetPos !== currentSelected) {
                                    const delta = targetPos - currentSelected;
                                    const dir = delta > 0 ? 'Down' : 'Up';
                                    for (let k = 0; k < Math.abs(delta); k++) keys.push(dir);
                                  }
                                  keys.push('Enter');
                                  await tmuxSendRawKeys(sessionName, windowId, pane, keys);
                                  await new Promise(r => setTimeout(r, 500));
                                  await tmuxSendKeys(sessionName, windowId, pane, text, 'Enter');
                                }}
                                className="px-3 py-1.5 border border-green-700/50 text-green-600 hover:text-green-400 hover:border-green-500 hover:bg-green-900/20 text-xs font-mono transition-colors"
                              >
                                SEND
                              </button>
                            </div>
                          );
                        }

                        return (
                        <button
                          key={opt.index}
                          onMouseEnter={() => !menu.multi_select && navigateToOption(opt.index)}
                          onClick={() => {
                            if (menu.multi_select) {
                              toggleOption(opt.index);
                            } else if (hasDesc && !isExpanded) {
                              setMenuExpandedOpt(opt.index);
                              navigateToOption(opt.index);
                            } else {
                              selectOption(opt.index);
                            }
                          }}
                          onDoubleClick={() => {
                            if (!menu.multi_select) selectOption(opt.index);
                          }}
                          className={`text-left p-2 border font-mono text-xs sm:text-sm transition-colors ${
                            isHighlighted
                              ? 'border-green-500 bg-green-900/20 text-green-400'
                              : isChatAbout
                                ? 'border-dashed border-green-900/50 text-green-700 hover:text-green-500 hover:border-green-700 cursor-pointer'
                                : 'border-green-700/50 text-green-400 hover:border-green-500 hover:bg-green-900/20 cursor-pointer'
                          }`}
                        >
                          {menu.multi_select && (
                            <span className={`mr-2 ${opt.checked ? 'text-green-400' : 'text-green-800'}`}>
                              {opt.checked ? '[✓]' : '[ ]'}
                            </span>
                          )}
                          {!menu.multi_select && <span className="text-green-600 mr-2">{opt.index}.</span>}
                          <span className={isChatAbout ? '' : 'font-bold'}>{opt.label}</span>
                          {hasDesc && !isExpanded && <span className="text-green-800 ml-2 text-[10px]">▸ 点击展开</span>}
                          {hasDesc && isExpanded && (
                            <span className="block text-[10px] sm:text-xs text-green-700 mt-1 ml-4">{opt.description}</span>
                          )}
                          {isExpanded && <span className="block text-green-500 text-[10px] mt-1 ml-4">▸ 再次点击确认选择</span>}
                        </button>
                        );
                      })}
                      {/* Submit button for multi-select */}
                      {menu.multi_select && (
                        <button
                          onClick={submitMultiSelect}
                          className="mt-1 p-2 border border-green-600/70 bg-green-900/30 text-green-400 font-mono text-xs sm:text-sm hover:bg-green-800/30 hover:border-green-500 transition-colors font-bold"
                        >
                          ✓ Submit
                        </button>
                      )}
                    </div>
                    {/* Right: preview panel */}
                    {currentPreview && (
                      <div className="flex-1 min-w-0 bg-black/40 border border-green-900/50 p-3 overflow-auto max-h-52">
                        <pre className="text-[10px] sm:text-xs text-green-300/80 font-mono whitespace-pre-wrap break-words leading-relaxed">
                          {currentPreview}
                        </pre>
                      </div>
                    )}
                  </div>
                </div>
              );
            })()}
        </div>

        {/* Input Area — only shown for live sessions */}
        {isLive && sessionName && windowId && (
          <div className="p-2 sm:p-3 border-t border-green-800 bg-green-900/10 flex-shrink-0">
            {/* Image previews */}
            {pendingImages.length > 0 && (
              <div className="mb-2 flex flex-wrap items-start gap-2 p-2 border border-green-800 bg-green-900/20">
                {pendingImages.map((img, idx) => (
                  <div key={img.preview || img.file.name} className="relative group">
                    {img.preview ? (
                      <img
                        src={img.preview}
                        alt={`Preview ${idx + 1}`}
                        className="h-16 w-16 object-cover border border-green-800 rounded"
                      />
                    ) : (
                      <div className="h-16 w-16 flex flex-col items-center justify-center border border-green-800 rounded bg-green-900/30 text-green-500">
                        <span className="text-lg">📄</span>
                        <span className="text-[8px] font-mono truncate w-14 text-center">{img.file.name.split('.').pop()?.toUpperCase()}</span>
                      </div>
                    )}
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
                title="Attach file (image, PDF, DOC)"
              >
                <Paperclip className="w-4 h-4" />
              </button>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*,.pdf,.doc,.docx,application/pdf,application/msword,application/vnd.openxmlformats-officedocument.wordprocessingml.document"
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
                        <span className="text-yellow-500 animate-pulse">
                            ⏳ {uploadPercent !== null && uploadPercent < 100
                                ? `UPLOADING ${uploadPercent}%`
                                : uploadPercent === 100
                                    ? 'PROCESSING...'
                                    : 'SENDING...'}
                        </span>
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
