import React, { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { X, MessageSquare, FileText, Wrench, GitCommit, Clock, Check, XCircle, ChevronRight, Search, ChevronUp, ChevronDown } from 'lucide-react';
import { fetchHistoryDetail, fetchSessionDetail, fetchGroupedDetail, HistoryDetail, ConversationMessage, ToolUsageRecord, GitCommitRecord, GroupedDetailResponse, TaskSegment, GroupedMessage, formatDuration, TimelineEntry } from '../services/api';
import { ChatTimeline, fromHistoryTimeline } from './ChatTimeline';
import { useSearch } from '../hooks/useSearch';
import { SearchHighlight, countMatches } from './SearchHighlight';
import { MarkdownText } from './MarkdownText';

interface HistoryDetailModalProps {
  historyId: number;
  filePath?: string;
  groupIds?: number[];
  projectGitDir?: string;
  onClose: () => void;
  isOpen: boolean;
}

type TabId = 'messages' | 'summary' | 'tools' | 'commits';

const TABS: { id: TabId; label: string; icon: React.ReactNode }[] = [
  { id: 'messages', label: '对话', icon: <MessageSquare className="w-4 h-4" /> },
  { id: 'summary', label: '摘要', icon: <FileText className="w-4 h-4" /> },
  { id: 'tools', label: '工具', icon: <Wrench className="w-4 h-4" /> },
  { id: 'commits', label: '提交', icon: <GitCommit className="w-4 h-4" /> },
];

export const HistoryDetailModal: React.FC<HistoryDetailModalProps> = ({
  historyId,
  filePath,
  groupIds,
  projectGitDir,
  onClose,
  isOpen,
}) => {
  const [activeTab, setActiveTab] = useState<TabId>('messages');
  const [detail, setDetail] = useState<HistoryDetail | null>(null);
  const [groupedDetail, setGroupedDetail] = useState<GroupedDetailResponse | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const search = useSearch();

  // Debounced search query — input stays responsive, highlighting runs after user stops typing
  const [debouncedQuery, setDebouncedQuery] = useState('');
  useEffect(() => {
    if (!search.query) {
      setDebouncedQuery('');
      return;
    }
    const timer = setTimeout(() => setDebouncedQuery(search.query), 300);
    return () => clearTimeout(timer);
  }, [search.query]);

  const isGroupedMode = !!(groupIds && groupIds.length > 0);

  // Collect all searchable text blocks for the current tab
  const searchableTexts = useMemo(() => {
    if (!debouncedQuery) return [];
    // Grouped mode
    if (isGroupedMode && groupedDetail) {
      switch (activeTab) {
        case 'messages':
          return (groupedDetail.messages || []).filter(m => m.content.trim().length > 0).map(m => m.content);
        case 'summary':
          return (groupedDetail.segments || []).map(s => s.summary).filter(Boolean);
        case 'tools':
          return (groupedDetail.tool_usage || []).flatMap(t => [t.tool_name, t.tool_args, t.result_summary].filter(Boolean));
        case 'commits':
          return (groupedDetail.commits || []).flatMap(c => [c.commit_hash, c.commit_message].filter(Boolean));
        default:
          return [];
      }
    }
    // Single entry mode
    if (!detail) return [];
    switch (activeTab) {
      case 'messages':
        return (detail.messages || []).filter(m => m.content.trim().length > 0).map(m => m.content);
      case 'summary':
        return [detail.summary, detail.completion_note, detail.resume_command].filter(Boolean) as string[];
      case 'tools':
        return (detail.tool_usage || []).flatMap(t => [t.tool_name, t.tool_args, t.result_summary].filter(Boolean));
      case 'commits':
        return (detail.commits || []).flatMap(c => [c.commit_hash, c.commit_message].filter(Boolean));
      default:
        return [];
    }
  }, [detail, groupedDetail, isGroupedMode, activeTab, debouncedQuery]);

  // Pre-compute match info in a single O(n) pass — replaces the O(n²) getStartMatchIndex
  const matchInfo = useMemo(() => {
    if (!debouncedQuery || searchableTexts.length === 0) {
      return { starts: [] as number[], counts: [] as number[], total: 0 };
    }
    const starts: number[] = [];
    const counts: number[] = [];
    let cumulative = 0;
    for (const text of searchableTexts) {
      starts.push(cumulative);
      const c = countMatches(text, debouncedQuery);
      counts.push(c);
      cumulative += c;
    }
    return { starts, counts, total: cumulative };
  }, [searchableTexts, debouncedQuery]);

  // Update search state with total match count
  useEffect(() => {
    search.resetMatches(matchInfo.total);
  }, [matchInfo.total]);

  // Find which message contains the current match — for message-level scrolling
  const activeMessageIndex = useMemo(() => {
    if (matchInfo.total === 0 || search.currentIndex < 0) return -1;
    const idx = search.currentIndex;
    for (let i = 0; i < matchInfo.starts.length; i++) {
      if (idx < matchInfo.starts[i] + matchInfo.counts[i]) return i;
    }
    return -1;
  }, [matchInfo, search.currentIndex]);

  // Message-level refs for scroll navigation (instead of per-highlight refs)
  const messageRefs = useRef<Map<number, HTMLElement>>(new Map());
  const registerMessageRef = useCallback((index: number, el: HTMLElement | null) => {
    if (el) messageRefs.current.set(index, el);
    else messageRefs.current.delete(index);
  }, []);

  // Scroll to the message containing the current match
  useEffect(() => {
    if (activeMessageIndex >= 0) {
      const el = messageRefs.current.get(activeMessageIndex);
      if (el) {
        el.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }
    }
  }, [activeMessageIndex, search.currentIndex]);

  // Focus search input when activated
  useEffect(() => {
    if (search.isActive) {
      searchInputRef.current?.focus();
    }
  }, [search.isActive]);

  // Close search when switching tabs
  useEffect(() => {
    if (search.isActive) {
      search.setQuery('');
      search.resetMatches(0);
    }
  }, [activeTab]);

  // Fetch detail data
  useEffect(() => {
    if (!isOpen) return;

    const loadDetail = async () => {
      setIsLoading(true);
      setError(null);
      setDetail(null);
      setGroupedDetail(null);
      try {
        if (isGroupedMode && projectGitDir) {
          // Grouped mode: fetch merged detail from multiple history entries
          const data = await fetchGroupedDetail(projectGitDir, groupIds!);
          setGroupedDetail(data);
        } else if (filePath) {
          const data = await fetchSessionDetail(filePath);
          setDetail(data);
        } else if (historyId) {
          const data = await fetchHistoryDetail(historyId);
          setDetail(data);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load detail');
      } finally {
        setIsLoading(false);
      }
    };

    loadDetail();
  }, [historyId, filePath, groupIds, projectGitDir, isOpen, isGroupedMode]);

  // Keyboard shortcuts
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const isInSearchInput = target === searchInputRef.current;

      // Prevent all keyboard events from bubbling to main page
      e.stopPropagation();

      // When search input is focused, only handle Escape and Enter
      if (isInSearchInput) {
        if (e.key === 'Escape') {
          e.preventDefault();
          search.close();
          return;
        }
        if (e.key === 'Enter') {
          e.preventDefault();
          if (e.shiftKey) {
            search.prev();
          } else {
            search.next();
          }
          return;
        }
        return; // Let normal typing work
      }

      switch (e.key) {
        case 'Escape':
        case 'q':
          e.preventDefault();
          if (search.isActive) {
            search.close();
          } else {
            onClose();
          }
          break;
        case '/':
          e.preventDefault();
          search.open();
          break;
        case 'n':
          e.preventDefault();
          if (search.isActive) {
            search.next();
          }
          break;
        case 'N':
          e.preventDefault();
          if (search.isActive) {
            search.prev();
          }
          break;
        case '1':
          e.preventDefault();
          setActiveTab('messages');
          break;
        case '2':
          e.preventDefault();
          setActiveTab('summary');
          break;
        case '3':
          e.preventDefault();
          setActiveTab('tools');
          break;
        case '4':
          e.preventDefault();
          setActiveTab('commits');
          break;
        case 'j':
        case 'J':
          e.preventDefault();
          if (contentRef.current) {
            contentRef.current.scrollBy({ top: 100, behavior: 'smooth' });
          }
          break;
        case 'k':
        case 'K':
          e.preventDefault();
          if (contentRef.current) {
            contentRef.current.scrollBy({ top: -100, behavior: 'smooth' });
          }
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown, true); // Use capture phase
    return () => window.removeEventListener('keydown', handleKeyDown, true);
  }, [isOpen, onClose, search]);

  if (!isOpen) return null;

  // Render grouped messages with task boundary dividers
  const renderGroupedMessages = (data: GroupedDetailResponse) => {
    if (!data.messages || data.messages.length === 0) {
      return (
        <div className="text-green-700 italic space-y-2 p-4">
          <div>暂无对话记录</div>
          <div className="text-green-800 text-xs">可能原因: Claude JSONL 会话文件已被清理或时间戳不匹配</div>
        </div>
      );
    }

    const filtered = data.messages.filter(m => m.content.trim().length > 0);
    if (filtered.length === 0) {
      return <div className="text-green-700 italic">暂无对话记录</div>;
    }

    // Build a map from history_id to segment info
    const segmentMap = new Map<number, TaskSegment>();
    for (const seg of data.segments) {
      segmentMap.set(seg.history_id, seg);
    }

    let lastHistoryId: number | null = null;

    return (
      <div className="space-y-3">
        {filtered.map((msg, index) => {
          const content = msg.content.length > 2000 ? msg.content.slice(0, 2000) + '...' : msg.content;
          const hasMatch = (matchInfo.counts[index] ?? 0) > 0;
          const isActive = index === activeMessageIndex;
          const showDivider = msg.history_id !== lastHistoryId;
          const segment = showDivider ? segmentMap.get(msg.history_id) : null;
          lastHistoryId = msg.history_id;
          const isUser = msg.role === 'user';

          return (
            <React.Fragment key={index}>
              {showDivider && segment && (
                <div className="flex items-center gap-3 py-2 my-2">
                  <div className="flex-1 h-px bg-cyan-800/50"></div>
                  <div className="text-cyan-500 text-xs font-mono flex items-center gap-2 px-3 py-1 bg-cyan-900/20 border border-cyan-800/30 rounded">
                    <span className="text-cyan-300 font-bold">
                      {segment.summary.length > 60 ? segment.summary.slice(0, 60) + '...' : segment.summary}
                    </span>
                    {segment.started_at && (
                      <span className="text-cyan-700">
                        {new Date(segment.started_at).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false })}
                      </span>
                    )}
                  </div>
                  <div className="flex-1 h-px bg-cyan-800/50"></div>
                </div>
              )}
              <div
                ref={(el) => registerMessageRef(index, el)}
                className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}
              >
                <div
                  className={`max-w-[85%] rounded-lg px-4 py-3 overflow-hidden break-words ${
                    isUser
                      ? 'bg-blue-600/25 border border-blue-500/40 rounded-br-sm'
                      : 'bg-gray-800/60 border border-gray-700/50 rounded-bl-sm'
                  } ${isActive ? 'ring-1 ring-yellow-400/50' : ''}`}
                >
                  <div className={`flex items-center gap-2 mb-1.5 ${isUser ? 'justify-end' : 'justify-start'}`}>
                    <span
                      className={`text-[10px] font-bold uppercase px-1.5 py-0.5 rounded ${
                        isUser
                          ? 'bg-blue-500/20 text-blue-400'
                          : 'bg-green-500/20 text-green-400'
                      }`}
                    >
                      {isUser ? 'YOU' : 'AI'}
                    </span>
                    {msg.created_at && (
                      <span className="text-[10px] text-gray-500">
                        {new Date(msg.created_at).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false })}
                      </span>
                    )}
                  </div>
                  <div className={`text-sm font-mono leading-relaxed ${isUser ? 'text-blue-200' : 'text-green-300'}`}>
                    <MarkdownText
                      content={content}
                      searchQuery={hasMatch ? debouncedQuery : undefined}
                      searchCurrentIndex={isActive ? search.currentIndex : -1}
                      searchStartMatchIndex={hasMatch ? (matchInfo.starts[index] ?? 0) : undefined}
                    />
                  </div>
                </div>
              </div>
            </React.Fragment>
          );
        })}
      </div>
    );
  };

  const renderMessages = (messages: ConversationMessage[]) => {
    if (!messages || messages.length === 0) {
      return (
        <div className="text-green-700 italic space-y-2 p-4">
          <div>暂无对话记录</div>
          <div className="text-green-800 text-xs">可能原因: 该条目创建时未保存对话内容，或会话文件已被清理</div>
        </div>
      );
    }

    // Filter out empty messages (tool-only assistant turns with no text)
    const filtered = messages.filter(m => m.content.trim().length > 0);
    if (filtered.length === 0) {
      return (
        <div className="text-green-700 italic space-y-2 p-4">
          <div>暂无对话记录</div>
          <div className="text-green-800 text-xs">该条目仅包含工具调用，无文字对话内容</div>
        </div>
      );
    }

    return (
      <div className="space-y-3">
        {filtered.map((msg, index) => {
          const content = msg.content.length > 2000 ? msg.content.slice(0, 2000) + '...' : msg.content;
          const hasMatch = (matchInfo.counts[index] ?? 0) > 0;
          const isActive = index === activeMessageIndex;
          const isUser = msg.role === 'user';

          return (
            <div
              key={index}
              ref={(el) => registerMessageRef(index, el)}
              className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}
            >
              <div
                className={`max-w-[85%] rounded-lg px-4 py-3 ${
                  isUser
                    ? 'bg-blue-600/25 border border-blue-500/40 rounded-br-sm'
                    : 'bg-gray-800/60 border border-gray-700/50 rounded-bl-sm'
                } ${isActive ? 'ring-1 ring-yellow-400/50' : ''}`}
              >
                <div className={`flex items-center gap-2 mb-1.5 ${isUser ? 'justify-end' : 'justify-start'}`}>
                  <span
                    className={`text-[10px] font-bold uppercase px-1.5 py-0.5 rounded ${
                      isUser
                        ? 'bg-blue-500/20 text-blue-400'
                        : 'bg-green-500/20 text-green-400'
                    }`}
                  >
                    {isUser ? 'YOU' : 'AI'}
                  </span>
                  {msg.created_at && (
                    <span className="text-[10px] text-gray-500">
                      {new Date(msg.created_at).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false })}
                    </span>
                  )}
                </div>
                <div className={`text-sm font-mono leading-relaxed ${isUser ? 'text-blue-200' : 'text-green-300'}`}>
                  <MarkdownText
                    content={content}
                    searchQuery={hasMatch ? debouncedQuery : undefined}
                    searchCurrentIndex={isActive ? search.currentIndex : -1}
                    searchStartMatchIndex={hasMatch ? (matchInfo.starts[index] ?? 0) : undefined}
                  />
                </div>
              </div>
            </div>
          );
        })}
      </div>
    );
  };

  const renderSummary = () => {
    if (!detail) return null;

    return (
      <div className="space-y-6">
        {/* Basic Info */}
        <div className="grid grid-cols-2 gap-4">
          <div className="bg-green-900/20 border border-green-800/50 p-4 rounded">
            <div className="text-green-700 text-xs uppercase mb-1">Session</div>
            <div className="text-green-300 font-mono">{detail.session || '-'}</div>
          </div>
          <div className="bg-green-900/20 border border-green-800/50 p-4 rounded">
            <div className="text-green-700 text-xs uppercase mb-1">Window</div>
            <div className="text-green-300 font-mono">{detail.window || '-'}</div>
          </div>
          <div className="bg-green-900/20 border border-green-800/50 p-4 rounded">
            <div className="text-green-700 text-xs uppercase mb-1">开始时间</div>
            <div className="text-green-300 font-mono">
              {detail.started_at ? new Date(detail.started_at).toLocaleString() : '-'}
            </div>
          </div>
          <div className="bg-green-900/20 border border-green-800/50 p-4 rounded">
            <div className="text-green-700 text-xs uppercase mb-1">结束时间</div>
            <div className="text-green-300 font-mono">
              {detail.ended_at ? new Date(detail.ended_at).toLocaleString() : '-'}
            </div>
          </div>
        </div>

        {/* Stats */}
        {detail.stats && (
          <div className="bg-black/40 border border-green-800/50 p-4 rounded">
            <div className="text-green-500 font-bold mb-3 uppercase tracking-wider">统计信息</div>
            <div className="grid grid-cols-4 gap-4">
              <div className="text-center">
                <div className="text-2xl font-bold text-green-400">{detail.stats.message_count}</div>
                <div className="text-xs text-green-700">消息数</div>
              </div>
              <div className="text-center">
                <div className="text-2xl font-bold text-cyan-400">{detail.stats.tool_count}</div>
                <div className="text-xs text-green-700">工具调用</div>
              </div>
              <div className="text-center">
                <div className="text-2xl font-bold text-yellow-400">{detail.stats.commit_count}</div>
                <div className="text-xs text-green-700">Git 提交</div>
              </div>
              <div className="text-center">
                <div className="text-2xl font-bold text-green-400">
                  {formatDuration(detail.stats.duration_seconds)}
                </div>
                <div className="text-xs text-green-700">耗时</div>
              </div>
            </div>
          </div>
        )}

        {/* Tools Used */}
        {detail.stats?.tools_used && detail.stats.tools_used.length > 0 && (
          <div className="bg-black/40 border border-green-800/50 p-4 rounded">
            <div className="text-green-500 font-bold mb-3 uppercase tracking-wider">使用的工具</div>
            <div className="flex flex-wrap gap-2">
              {detail.stats.tools_used.map((tool, index) => (
                <span
                  key={index}
                  className="px-2 py-1 bg-cyan-900/30 border border-cyan-800/50 text-cyan-400 text-xs font-mono rounded"
                >
                  {tool}
                </span>
              ))}
            </div>
          </div>
        )}

        {/* Summary & Completion Note */}
        {detail.summary && (
          <div className="bg-black/40 border border-green-800/50 p-4 rounded">
            <div className="text-green-500 font-bold mb-2 uppercase tracking-wider">任务摘要</div>
            <div className="text-green-300 text-sm"><MarkdownText content={detail.summary} /></div>
          </div>
        )}

        {detail.completion_note && (
          <div className="bg-black/40 border border-green-800/50 p-4 rounded">
            <div className="text-green-500 font-bold mb-2 uppercase tracking-wider">完成备注</div>
            <div className="text-green-300 text-sm"><MarkdownText content={detail.completion_note} /></div>
          </div>
        )}

        {/* Resume Command */}
        {detail.resume_command && (
          <div className="bg-black/40 border border-green-800/50 p-4 rounded">
            <div className="text-green-500 font-bold mb-2 uppercase tracking-wider">恢复命令</div>
            <code className="text-cyan-400 text-sm font-mono block bg-black/50 p-2 rounded">
              {detail.resume_command}
            </code>
          </div>
        )}
      </div>
    );
  };

  const renderTools = (tools?: ToolUsageRecord[]) => {
    if (!tools || tools.length === 0) {
      return <div className="text-green-700 italic">暂无工具使用记录</div>;
    }

    return (
      <div className="space-y-3">
        {tools.map((tool, index) => (
          <div
            key={index}
            className={`p-3 rounded border ${
              tool.success
                ? 'bg-green-900/10 border-green-800/50'
                : 'bg-red-900/10 border-red-800/50'
            }`}
          >
            <div className="flex items-center gap-2 mb-2">
              {tool.success ? (
                <Check className="w-4 h-4 text-green-500" />
              ) : (
                <XCircle className="w-4 h-4 text-red-500" />
              )}
              <span className="font-bold text-green-400">{tool.tool_name}</span>
              {tool.timestamp && (
                <span className="text-xs text-green-700">
                  {new Date(tool.timestamp).toLocaleTimeString()}
                </span>
              )}
            </div>
            {tool.tool_args && (
              <details className="mb-2">
                <summary className="text-xs text-green-600 cursor-pointer hover:text-green-400">
                  参数
                </summary>
                <pre className="text-xs text-green-500 mt-1 p-2 bg-black/50 rounded overflow-x-hidden whitespace-pre-wrap break-all">
                  {tool.tool_args.length > 500 ? tool.tool_args.slice(0, 500) + '...' : tool.tool_args}
                </pre>
              </details>
            )}
            {tool.result_summary && (
              <div className="text-xs text-green-600 bg-black/30 p-2 rounded">
                {tool.result_summary}
              </div>
            )}
          </div>
        ))}
      </div>
    );
  };

  const renderCommits = (commits?: GitCommitRecord[]) => {
    if (!commits || commits.length === 0) {
      return <div className="text-green-700 italic">暂无 Git 提交记录</div>;
    }

    return (
      <div className="space-y-3">
        {commits.map((commit, index) => (
          <div
            key={index}
            className="p-4 rounded border border-yellow-800/50 bg-yellow-900/10"
          >
            <div className="flex items-start gap-3">
              <GitCommit className="w-5 h-5 text-yellow-500 flex-shrink-0 mt-0.5" />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <code className="text-yellow-400 font-mono text-sm">
                    {commit.commit_hash.slice(0, 7)}
                  </code>
                  {commit.files_changed > 0 && (
                    <span className="text-xs text-green-700">
                      {commit.files_changed} 文件
                    </span>
                  )}
                  {commit.timestamp && (
                    <span className="text-xs text-green-700">
                      {new Date(commit.timestamp).toLocaleTimeString()}
                    </span>
                  )}
                </div>
                <div className="text-green-300 text-sm">{commit.commit_message}</div>
              </div>
            </div>
          </div>
        ))}
      </div>
    );
  };

  // Render grouped summary (list of task segments)
  const renderGroupedSummary = (data: GroupedDetailResponse) => {
    if (!data.segments || data.segments.length === 0) {
      return <div className="text-green-700 italic">暂无摘要</div>;
    }

    return (
      <div className="space-y-4">
        <div className="bg-black/40 border border-green-800/50 p-4 rounded">
          <div className="text-green-500 font-bold mb-3 uppercase tracking-wider">
            任务列表 ({data.segments.length} 个任务)
          </div>
          <div className="space-y-3">
            {data.segments.map((seg, index) => (
              <div key={seg.history_id} className="flex items-start gap-3 p-3 bg-green-900/10 border border-green-800/30 rounded">
                <span className="text-cyan-500 font-mono text-sm flex-shrink-0">#{index + 1}</span>
                <div className="flex-1 min-w-0">
                  <div className="text-green-300 text-sm">{seg.summary}</div>
                  <div className="text-green-700 text-xs mt-1 flex items-center gap-3">
                    <span>{new Date(seg.started_at).toLocaleString()}</span>
                    <span>{seg.message_count} 条消息</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Aggregate stats */}
        <div className="bg-black/40 border border-green-800/50 p-4 rounded">
          <div className="text-green-500 font-bold mb-3 uppercase tracking-wider">统计信息</div>
          <div className="grid grid-cols-4 gap-4">
            <div className="text-center">
              <div className="text-2xl font-bold text-green-400">{data.messages?.length ?? 0}</div>
              <div className="text-xs text-green-700">消息数</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-cyan-400">{data.tool_usage?.length ?? 0}</div>
              <div className="text-xs text-green-700">工具调用</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-yellow-400">{data.commits?.length ?? 0}</div>
              <div className="text-xs text-green-700">Git 提交</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-green-400">{data.segments.length}</div>
              <div className="text-xs text-green-700">任务数</div>
            </div>
          </div>
        </div>
      </div>
    );
  };

  const renderTabContent = () => {
    if (isLoading) {
      return <div className="text-green-600 animate-pulse">LOADING...</div>;
    }

    if (error) {
      return <div className="text-red-500">Error: {error}</div>;
    }

    // Grouped mode
    if (isGroupedMode && groupedDetail) {
      switch (activeTab) {
        case 'messages':
          return renderGroupedMessages(groupedDetail);
        case 'summary':
          return renderGroupedSummary(groupedDetail);
        case 'tools':
          return renderTools(groupedDetail.tool_usage as ToolUsageRecord[]);
        case 'commits':
          return renderCommits(groupedDetail.commits as GitCommitRecord[]);
        default:
          return null;
      }
    }

    // Single entry mode
    if (!detail) {
      return (
        <div className="text-green-700 italic space-y-2 p-4">
          <div>无法加载详情</div>
          <div className="text-green-800 text-xs">关联的会话文件可能已被删除或移动</div>
        </div>
      );
    }

    switch (activeTab) {
      case 'messages':
        // Use rich timeline view when available (session detail), fallback to legacy messages
        if (detail.timeline && detail.timeline.length > 0) {
          return (
            <div className="font-mono">
              <ChatTimeline items={fromHistoryTimeline(detail.timeline)} />
            </div>
          );
        }
        return renderMessages(detail.messages || []);
      case 'summary':
        return renderSummary();
      case 'tools':
        return renderTools(detail.tool_usage);
      case 'commits':
        return renderCommits(detail.commits);
      default:
        return null;
    }
  };

  return (
    <div
      className="fixed inset-0 bg-black/90 z-[100] flex items-start justify-center p-4 pt-16 sm:pt-20 overflow-hidden overscroll-none"
      onClick={onClose}
      style={{ touchAction: 'none' }}
    >
      <div
        className="bg-black border-2 border-green-500 w-full max-w-4xl max-h-[90vh] flex flex-col shadow-[0_0_30px_rgba(34,197,94,0.4)] overflow-hidden"
        style={{ touchAction: 'pan-y' }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-green-800">
          <div className="flex items-center gap-3">
            <span className="text-green-500 font-bold uppercase tracking-wider">
              {isGroupedMode ? 'GROUPED DETAIL' : 'HISTORY DETAIL'}
            </span>
            {isGroupedMode && groupedDetail && groupedDetail.segments.length > 0 ? (
              <>
                <ChevronRight className="w-4 h-4 text-green-700" />
                <span className="text-green-400 font-mono">
                  {groupedDetail.segments[0].summary.length > 40
                    ? groupedDetail.segments[0].summary.slice(0, 40) + '...'
                    : groupedDetail.segments[0].summary}
                </span>
                <span className="text-cyan-600 text-sm">
                  {groupedDetail.segments.length} 个任务
                </span>
              </>
            ) : detail ? (
              <>
                <ChevronRight className="w-4 h-4 text-green-700" />
                <span className="text-green-400 font-mono">
                  {detail.window || detail.session}
                </span>
                {detail.stats && (
                  <span className="text-green-700 text-sm flex items-center gap-1">
                    <Clock className="w-3 h-3" />
                    {formatDuration(detail.stats.duration_seconds)}
                  </span>
                )}
              </>
            ) : null}
          </div>
          <button
            onClick={onClose}
            className="text-green-600 hover:text-green-400 transition-colors"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        {/* Tabs */}
        <div className="flex border-b border-green-800">
          {TABS.map((tab, index) => {
            // Get count for each tab
            const getCount = () => {
              if (isGroupedMode && groupedDetail) {
                switch (tab.id) {
                  case 'messages': return groupedDetail.messages?.length ?? 0;
                  case 'summary': return groupedDetail.segments?.length ?? 0;
                  case 'tools': return groupedDetail.tool_usage?.length ?? 0;
                  case 'commits': return groupedDetail.commits?.length ?? 0;
                  default: return 0;
                }
              }
              if (!detail) return 0;
              switch (tab.id) {
                case 'messages': return detail.messages?.length ?? 0;
                case 'summary': return 1;
                case 'tools': return detail.tool_usage?.length ?? 0;
                case 'commits': return detail.commits?.length ?? 0;
                default: return 0;
              }
            };
            const count = getCount();

            return (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2 px-4 py-3 font-mono text-sm transition-colors ${
                  activeTab === tab.id
                    ? 'text-green-400 bg-green-900/30 border-b-2 border-green-400'
                    : 'text-green-600 hover:text-green-400 hover:bg-green-900/10'
                }`}
                title={`快捷键: ${index + 1}`}
              >
                {tab.icon}
                <span>{tab.label}</span>
                <span className={`text-xs ${count > 0 ? 'text-cyan-500' : 'text-green-800'}`}>
                  [{count}]
                </span>
              </button>
            );
          })}
        </div>

        {/* Search Bar */}
        {search.isActive && (
          <div className="flex items-center gap-2 px-4 py-2 border-b border-green-800 bg-green-900/10">
            <Search className="w-4 h-4 text-green-600 flex-shrink-0" />
            <input
              ref={searchInputRef}
              type="text"
              value={search.query}
              onChange={(e) => search.setQuery(e.target.value)}
              placeholder="搜索..."
              className="flex-1 bg-transparent text-green-300 text-sm font-mono outline-none placeholder-green-700"
              autoFocus
            />
            {search.query && (
              <div className="flex items-center gap-1 flex-shrink-0">
                <span className="text-xs text-green-600 font-mono">
                  {search.matchCount > 0 ? `${search.currentIndex + 1}/${search.matchCount}` : '0/0'}
                </span>
                <button
                  onClick={search.prev}
                  className="p-0.5 text-green-600 hover:text-green-400 transition-colors"
                  title="上一个 (N)"
                >
                  <ChevronUp className="w-4 h-4" />
                </button>
                <button
                  onClick={search.next}
                  className="p-0.5 text-green-600 hover:text-green-400 transition-colors"
                  title="下一个 (n)"
                >
                  <ChevronDown className="w-4 h-4" />
                </button>
                <button
                  onClick={search.close}
                  className="p-0.5 text-green-600 hover:text-green-400 transition-colors ml-1"
                  title="关闭 (Esc)"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>
            )}
          </div>
        )}

        {/* Content */}
        <div ref={contentRef} className="flex-1 overflow-y-auto overflow-x-hidden p-4 min-h-[200px]">{renderTabContent()}</div>

        {/* Footer */}
        <div className="border-t border-green-800 px-4 py-2 text-xs text-green-700 font-mono flex justify-between">
          <span className="hidden sm:inline">SCROLL: [J/K] | TAB: [1/2/3/4] | SEARCH: [/] | CLOSE: [ESC]</span>
          <span className="sm:hidden">TAB: [1-4] | [ESC]</span>
          {detail?.transcript_path && (
            <span className="hidden md:inline text-green-800 truncate max-w-[300px]">
              {detail.transcript_path}
            </span>
          )}
        </div>
      </div>
    </div>
  );
};
