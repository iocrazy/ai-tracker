import React, { useState, useEffect, useCallback, useRef } from 'react';
import { X, MessageSquare, FileText, Wrench, GitCommit, Clock, Check, XCircle, ChevronRight } from 'lucide-react';
import { fetchHistoryDetail, HistoryDetail, ConversationMessage, ToolUsageRecord, GitCommitRecord, formatDuration } from '../services/api';

interface HistoryDetailModalProps {
  historyId: number;
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
  onClose,
  isOpen,
}) => {
  const [activeTab, setActiveTab] = useState<TabId>('messages');
  const [detail, setDetail] = useState<HistoryDetail | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);

  // Fetch detail data
  useEffect(() => {
    if (!isOpen || !historyId) return;

    const loadDetail = async () => {
      setIsLoading(true);
      setError(null);
      try {
        const data = await fetchHistoryDetail(historyId);
        setDetail(data);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load detail');
      } finally {
        setIsLoading(false);
      }
    };

    loadDetail();
  }, [historyId, isOpen]);

  // Keyboard shortcuts
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Prevent all keyboard events from bubbling to main page
      e.stopPropagation();

      switch (e.key) {
        case 'Escape':
          e.preventDefault();
          onClose();
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
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const renderMessages = (messages: ConversationMessage[]) => {
    if (!messages || messages.length === 0) {
      return <div className="text-green-700 italic">暂无对话记录</div>;
    }

    return (
      <div className="space-y-4">
        {messages.map((msg, index) => (
          <div
            key={index}
            className={`p-4 rounded border ${
              msg.role === 'user'
                ? 'bg-blue-900/20 border-blue-800/50'
                : 'bg-green-900/20 border-green-800/50'
            }`}
          >
            <div className="flex items-center gap-2 mb-2">
              <span
                className={`text-xs font-bold uppercase px-2 py-0.5 rounded ${
                  msg.role === 'user'
                    ? 'bg-blue-500/20 text-blue-400'
                    : 'bg-green-500/20 text-green-400'
                }`}
              >
                {msg.role === 'user' ? 'USER' : 'ASSISTANT'}
              </span>
              {msg.created_at && (
                <span className="text-xs text-green-700">
                  {new Date(msg.created_at).toLocaleTimeString()}
                </span>
              )}
            </div>
            <div className="text-green-300 text-sm whitespace-pre-wrap font-mono leading-relaxed">
              {msg.content.length > 2000 ? msg.content.slice(0, 2000) + '...' : msg.content}
            </div>
          </div>
        ))}
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
            <div className="text-green-300 text-sm">{detail.summary}</div>
          </div>
        )}

        {detail.completion_note && (
          <div className="bg-black/40 border border-green-800/50 p-4 rounded">
            <div className="text-green-500 font-bold mb-2 uppercase tracking-wider">完成备注</div>
            <div className="text-green-300 text-sm">{detail.completion_note}</div>
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
                <pre className="text-xs text-green-500 mt-1 p-2 bg-black/50 rounded overflow-x-auto">
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

  const renderTabContent = () => {
    if (isLoading) {
      return <div className="text-green-600 animate-pulse">LOADING...</div>;
    }

    if (error) {
      return <div className="text-red-500">Error: {error}</div>;
    }

    if (!detail) {
      return <div className="text-green-700">No data</div>;
    }

    switch (activeTab) {
      case 'messages':
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
      className="fixed inset-0 bg-black/90 z-[100] flex items-start justify-center p-4 pt-16 sm:pt-20"
      onClick={onClose}
    >
      <div
        className="bg-black border-2 border-green-500 w-full max-w-4xl max-h-[90vh] flex flex-col shadow-[0_0_30px_rgba(34,197,94,0.4)]"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-green-800">
          <div className="flex items-center gap-3">
            <span className="text-green-500 font-bold uppercase tracking-wider">
              HISTORY DETAIL
            </span>
            {detail && (
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
            )}
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
              if (!detail) return 0;
              switch (tab.id) {
                case 'messages': return detail.messages?.length ?? 0;
                case 'summary': return 1; // Summary is always 1 if detail exists
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

        {/* Content */}
        <div ref={contentRef} className="flex-1 overflow-y-auto p-4 min-h-[200px]">{renderTabContent()}</div>

        {/* Footer */}
        <div className="border-t border-green-800 px-4 py-2 text-xs text-green-700 font-mono flex justify-between">
          <span className="hidden sm:inline">SCROLL: [J/K] | TAB: [1/2/3/4] | CLOSE: [ESC]</span>
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
