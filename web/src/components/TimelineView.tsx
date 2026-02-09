import React, { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { TimelineEvent } from '../types';
import { Search, X, RefreshCw, Download, ChevronDown, ChevronLeft, ChevronRight, MessageSquare, Wrench, FileSearch } from 'lucide-react';
import { fetchHistory, fetchSessions, HistoryQueryParams, HistoryResponse, HistoryEntry, exportHistory } from '../services/api';
import { SearchHighlight } from './SearchHighlight';
import { MarkdownText } from './MarkdownText';

interface TimelineViewProps {
  events: TimelineEvent[];
  onViewDetails: (event: TimelineEvent) => void;
  isActive: boolean;
}

type TimeRange = 'today' | 'yesterday' | '7days' | '30days' | 'all';

const TIME_RANGE_LABELS: Record<TimeRange, string> = {
  today: '今天',
  yesterday: '昨天',
  '7days': '最近 7 天',
  '30days': '最近 30 天',
  all: '全部',
};

export const TimelineView: React.FC<TimelineViewProps> = ({ events: propEvents, onViewDetails, isActive }) => {
  const [search, setSearch] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [showHelp, setShowHelp] = useState(false);
  const [timeRange, setTimeRange] = useState<TimeRange>('today');
  const [showTimeRangeDropdown, setShowTimeRangeDropdown] = useState(false);
  const [isLoading, setIsLoading] = useState(false);

  // Pagination
  const [page, setPage] = useState(1);
  const [perPage] = useState(50);
  const [total, setTotal] = useState(0);

  // Deep search (server-side full-text)
  const [deepQuery, setDeepQuery] = useState('');
  const [deepSearchInput, setDeepSearchInput] = useState('');
  const [isDeepSearchOpen, setIsDeepSearchOpen] = useState(false);

  // Fetched history data
  const [historyData, setHistoryData] = useState<HistoryResponse | null>(null);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const deepSearchInputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLDivElement | null)[]>([]);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Convert HistoryEntry to TimelineEvent
  const convertToTimelineEvent = useCallback((entry: HistoryEntry): TimelineEvent => {
    const startTime = entry.started_at ? new Date(entry.started_at) : new Date();
    // Format: session:window (e.g., "1-tracker:main")
    const displayName = entry.session && entry.window
      ? `${entry.session}:${entry.window}`
      : entry.window || entry.session || 'Unknown';
    return {
      id: String(entry.id),
      time: startTime.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false }),
      user: displayName,
      action: 'COMPLETED',
      description: entry.summary || entry.completion_note || 'No description',
      // Store extra data for detail view
      historyId: entry.id,
      filePath: entry.file_path,
      messageCount: entry.message_count,
      duration: entry.duration_seconds,
    };
  }, []);

  // Deep search handlers
  const handleDeepSearch = useCallback(() => {
    setDeepQuery(deepSearchInput);
    setPage(1);
  }, [deepSearchInput]);

  const closeDeepSearch = useCallback(() => {
    setIsDeepSearchOpen(false);
    setDeepSearchInput('');
    setDeepQuery('');
  }, []);

  // Fetch history data
  const loadHistory = useCallback(async () => {
    setIsLoading(true);
    try {
      const params: HistoryQueryParams = {
        range: timeRange,
        page,
        per_page: perPage,
      };
      if (deepQuery) {
        params.search = deepQuery;
      }
      const data = await fetchSessions(params);
      setHistoryData(data);
      setTotal(data.total);
    } catch (error) {
      console.error('Failed to fetch history:', error);
    } finally {
      setIsLoading(false);
    }
  }, [timeRange, page, perPage, deepQuery]);

  // Initial load and refresh on filter change
  useEffect(() => {
    loadHistory();
  }, [loadHistory]);

  // Convert history data to events
  const events = useMemo(() => {
    if (!historyData) return propEvents;

    const allEntries: HistoryEntry[] = [];
    for (const group of historyData.groups) {
      allEntries.push(...group.records);
    }
    return allEntries.map(convertToTimelineEvent);
  }, [historyData, propEvents, convertToTimelineEvent]);

  // Filter events based on search query (local filter for already fetched data)
  const filteredEvents = useMemo(() => {
    if (!search) return events;
    return events.filter(e =>
      e.description.toLowerCase().includes(search.toLowerCase()) ||
      e.user.toLowerCase().includes(search.toLowerCase()) ||
      e.action.toLowerCase().includes(search.toLowerCase())
    );
  }, [events, search]);

  // Reset selection when filter changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [search, timeRange, page]);

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setShowTimeRangeDropdown(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Handle export
  const handleExport = useCallback(async () => {
    try {
      const params: HistoryQueryParams = { range: timeRange };
      if (deepQuery) params.search = deepQuery;
      const blob = await exportHistory(params, 'json');
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `history-${timeRange}-${new Date().toISOString().split('T')[0]}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (error) {
      console.error('Export failed:', error);
    }
  }, [timeRange, deepQuery]);

  // Handle Keyboard Shortcuts
  useEffect(() => {
    if (!isActive) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Ignore navigation keys if typing in search inputs
      const isInFilter = document.activeElement === searchInputRef.current;
      const isInDeepSearch = document.activeElement === deepSearchInputRef.current;
      if ((isInFilter || isInDeepSearch) && e.key !== 'Escape' && e.key !== 'Enter') {
        return;
      }

      // Enter in deep search submits
      if (isInDeepSearch && e.key === 'Enter') {
        e.preventDefault();
        handleDeepSearch();
        deepSearchInputRef.current?.blur();
        return;
      }

      // Shift+? to toggle help
      if (e.shiftKey && e.key === '?') {
        e.preventDefault();
        setShowHelp(prev => !prev);
        return;
      }

      switch (e.key) {
        case 'j':
        case 'ArrowDown':
          e.preventDefault();
          setSelectedIndex(prev => Math.min(prev + 1, filteredEvents.length - 1));
          break;
        case 'k':
        case 'ArrowUp':
          e.preventDefault();
          setSelectedIndex(prev => Math.max(prev - 1, 0));
          break;
        case '/':
          e.preventDefault();
          searchInputRef.current?.focus();
          break;
        case 's':
          e.preventDefault();
          setIsDeepSearchOpen(true);
          setTimeout(() => deepSearchInputRef.current?.focus(), 0);
          break;
        case 'l':
        case 'Enter':
          e.preventDefault();
          if (filteredEvents[selectedIndex]) {
            onViewDetails(filteredEvents[selectedIndex]);
            searchInputRef.current?.blur();
          }
          break;
        case 'r':
          e.preventDefault();
          loadHistory();
          break;
        case 'e':
          e.preventDefault();
          handleExport();
          break;
        case 'Escape':
          if (showHelp) {
            setShowHelp(false);
          } else if (isInDeepSearch) {
            deepSearchInputRef.current?.blur();
            if (!deepQuery) {
              setIsDeepSearchOpen(false);
            }
          } else if (isDeepSearchOpen && deepQuery) {
            closeDeepSearch();
          } else if (isDeepSearchOpen) {
            setIsDeepSearchOpen(false);
          } else if (showTimeRangeDropdown) {
            setShowTimeRangeDropdown(false);
          } else if (isInFilter) {
            searchInputRef.current?.blur();
          }
          break;
        // Page navigation
        case 'n':
          if (page < Math.ceil(total / perPage)) {
            setPage(p => p + 1);
          }
          break;
        case 'p':
          if (page > 1) {
            setPage(p => p - 1);
          }
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isActive, filteredEvents, selectedIndex, onViewDetails, showHelp, showTimeRangeDropdown, isDeepSearchOpen, deepQuery, loadHistory, handleDeepSearch, closeDeepSearch, handleExport, page, total, perPage]);

  // Auto-scroll to selected item
  useEffect(() => {
    const el = itemRefs.current[selectedIndex];
    if (el) {
      el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, [selectedIndex]);

  const totalPages = Math.ceil(total / perPage);

  return (
    <div className="retro-border bg-black/40 p-1 flex flex-col relative h-full overflow-hidden">
        {/* Header - Sticky */}
        <div className="p-3 sm:p-6 pb-3 sm:pb-4 border-b border-green-900/30 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sticky top-0 bg-black/95 z-40 backdrop-blur-sm flex-shrink-0">
            {/* Time Range Dropdown */}
            <div className="relative" ref={dropdownRef}>
              <button
                onClick={() => setShowTimeRangeDropdown(!showTimeRangeDropdown)}
                className="bg-green-500 text-black text-xs sm:text-sm font-bold px-2 sm:px-3 py-1 font-pixel uppercase tracking-widest shadow-[0_0_10px_rgba(34,197,94,0.6)] flex items-center gap-2 hover:bg-green-400 transition-colors"
              >
                {TIME_RANGE_LABELS[timeRange]}
                <ChevronDown className="w-4 h-4" />
              </button>

              {showTimeRangeDropdown && (
                <div className="absolute top-full left-0 mt-1 bg-black border border-green-500 shadow-lg z-50">
                  {(Object.keys(TIME_RANGE_LABELS) as TimeRange[]).map((range) => (
                    <button
                      key={range}
                      onClick={() => {
                        setTimeRange(range);
                        setPage(1);
                        setShowTimeRangeDropdown(false);
                      }}
                      className={`block w-full text-left px-4 py-2 text-sm font-mono hover:bg-green-900/30 transition-colors ${
                        timeRange === range ? 'text-green-400 bg-green-900/20' : 'text-green-600'
                      }`}
                    >
                      {TIME_RANGE_LABELS[range]}
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div className="flex items-center gap-2 sm:gap-4 w-full sm:w-auto">
              {/* Search Box */}
              <div className="flex items-center gap-2 group relative flex-1 sm:flex-initial">
                  <Search className="w-4 sm:w-5 h-4 sm:h-5 text-green-700" />
                  <input
                      ref={searchInputRef}
                      type="text"
                      value={search}
                      onChange={(e) => setSearch(e.target.value)}
                      placeholder="FILTER [/]"
                      className="bg-black border-b border-green-800 text-green-400 font-mono focus:outline-none focus:border-green-400 placeholder-green-900 w-full sm:w-48 md:w-64 py-1 text-sm sm:text-base"
                  />
                  {search && (
                    <button
                      onClick={() => setSearch('')}
                      className="absolute right-0 text-green-700 hover:text-green-400"
                    >
                      <X className="w-4 h-4" />
                    </button>
                  )}
              </div>

              {/* Action Buttons */}
              <button
                onClick={loadHistory}
                disabled={isLoading}
                className="p-2 text-green-600 hover:text-green-400 hover:bg-green-900/20 rounded transition-colors disabled:opacity-50"
                title="刷新 [R]"
              >
                <RefreshCw className={`w-4 h-4 ${isLoading ? 'animate-spin' : ''}`} />
              </button>
              <button
                onClick={handleExport}
                className="p-2 text-green-600 hover:text-green-400 hover:bg-green-900/20 rounded transition-colors"
                title="导出 [E]"
              >
                <Download className="w-4 h-4" />
              </button>
              <button
                onClick={() => {
                  setIsDeepSearchOpen(o => !o);
                  if (!isDeepSearchOpen) {
                    setTimeout(() => deepSearchInputRef.current?.focus(), 0);
                  }
                }}
                className={`p-2 rounded transition-colors ${
                  deepQuery
                    ? 'text-yellow-400 bg-yellow-900/20 hover:text-yellow-300'
                    : 'text-green-600 hover:text-green-400 hover:bg-green-900/20'
                }`}
                title="全文搜索 [S]"
              >
                <FileSearch className="w-4 h-4" />
              </button>
            </div>
        </div>

        {/* Deep Search Bar */}
        {isDeepSearchOpen && (
          <div className="px-3 sm:px-6 py-2 border-b border-green-900/30 bg-black/90 flex items-center gap-2 flex-shrink-0">
            <FileSearch className="w-4 h-4 text-yellow-500 flex-shrink-0" />
            <input
              ref={deepSearchInputRef}
              type="text"
              value={deepSearchInput}
              onChange={(e) => setDeepSearchInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  handleDeepSearch();
                }
              }}
              placeholder="全文搜索 summary + note ... [Enter 确认]"
              className="bg-black border-b border-yellow-800 text-yellow-400 font-mono focus:outline-none focus:border-yellow-400 placeholder-yellow-900/60 flex-1 py-1 text-sm"
            />
            {deepQuery && (
              <span className="text-yellow-600 text-xs font-mono flex-shrink-0">
                匹配: {filteredEvents.length}
              </span>
            )}
            <button
              onClick={closeDeepSearch}
              className="text-yellow-700 hover:text-yellow-400 flex-shrink-0"
              title="关闭搜索 [Esc]"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        )}

        {/* Full Page List Container */}
        <div ref={containerRef} className="flex-grow p-3 sm:p-6 pl-6 sm:pl-10 overflow-y-auto">
            <div className="relative border-l-2 border-green-800/30 pl-8 space-y-8 pb-10">
                {isLoading && filteredEvents.length === 0 ? (
                    <div className="text-green-600 font-mono p-4 animate-pulse">LOADING...</div>
                ) : filteredEvents.length === 0 ? (
                    <div className="text-green-800 font-mono italic p-4">NO_RECORDS_FOUND</div>
                ) : (
                    filteredEvents.map((event, index) => {
                        const isSelected = index === selectedIndex;
                        return (
                            <div
                                key={event.id}
                                ref={(el) => { itemRefs.current[index] = el; }}
                                onClick={() => {
                                    setSelectedIndex(index);
                                    onViewDetails(event);
                                }}
                                className={`relative group cursor-pointer transition-all duration-200 ${isSelected ? 'scale-[1.02] translate-x-2' : ''}`}
                            >
                                {/* Selection Indicator (Left Arrow) */}
                                {isSelected && (
                                    <div className="absolute -left-[60px] top-4 text-green-400 animate-pulse font-bold text-xl">
                                        ►
                                    </div>
                                )}

                                {/* Time Marker Dot */}
                                <div className={`
                                    absolute -left-[39px] top-1 w-5 h-5 rounded-full flex items-center justify-center border-2 transition-all z-10
                                    ${isSelected
                                        ? 'bg-green-500 border-green-300 scale-125 shadow-[0_0_15px_rgba(34,197,94,0.8)]'
                                        : 'bg-[#050505] border-cyan-400 shadow-[0_0_8px_rgba(34,211,238,0.5)]'
                                    }
                                `}>
                                    <div className={`w-2 h-2 rounded-full ${isSelected ? 'bg-white' : 'bg-cyan-400'}`}></div>
                                </div>

                                {/* Interactive Card */}
                                <div className={`
                                    p-4 rounded border transition-all -ml-4 pl-4
                                    ${isSelected
                                        ? 'bg-green-900/30 border-green-500 shadow-[inset_0_0_20px_rgba(34,197,94,0.1)]'
                                        : 'border-transparent hover:border-green-800/50 hover:bg-green-900/10'
                                    }
                                `}>
                                    <div className="flex flex-col md:flex-row md:items-start gap-2 md:gap-6">
                                        {/* Time */}
                                        <div className={`font-mono text-lg min-w-[60px] pt-0.5 transition-colors ${isSelected ? 'text-green-300 font-bold' : 'text-green-600'}`}>
                                            {event.time}
                                        </div>

                                        {/* Content */}
                                        <div className="flex-grow min-w-0">
                                            <div className="flex flex-wrap items-center gap-2 sm:gap-4 mb-2">
                                                <span className={`font-bold text-lg sm:text-xl tracking-wider ${isSelected ? 'text-white' : 'text-green-300'}`}>
                                                    {event.user}
                                                </span>
                                                <div className={`hidden sm:block h-px w-12 transition-colors ${isSelected ? 'bg-green-400' : 'bg-green-800'}`}></div>
                                                <span className={`font-bold tracking-widest uppercase text-xs sm:text-sm border px-2 py-0.5 transition-all
                                                    ${isSelected
                                                        ? 'text-green-900 bg-green-400 border-green-400'
                                                        : 'text-cyan-400 border-cyan-900/50 bg-cyan-900/10'
                                                    }
                                                `}>
                                                    {event.action}
                                                </span>

                                                {/* Stats badges */}
                                                {event.messageCount !== undefined && event.messageCount > 0 && (
                                                  <span className="flex items-center gap-1 text-xs text-green-700">
                                                    <MessageSquare className="w-3 h-3" />
                                                    {event.messageCount}
                                                  </span>
                                                )}
                                            </div>

                                            <div className={`text-base sm:text-lg font-sans tracking-wide leading-relaxed max-w-3xl transition-colors
                                                ${isSelected ? 'text-green-100' : 'text-green-500/80'}
                                            `}>
                                                <MarkdownText
                                                  content={event.description}
                                                  searchQuery={deepQuery}
                                                  onRegisterMatch={deepQuery ? () => {} : undefined}
                                                />
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        );
                    })
                )}

                {/* End Marker */}
                <div className="absolute bottom-0 left-[-1px] w-0.5 h-full bg-gradient-to-b from-green-800/30 to-transparent pointer-events-none"></div>
            </div>
        </div>

        {/* Pagination Footer */}
        {totalPages > 1 && (
          <div className="border-t border-green-900/30 px-4 py-2 flex items-center justify-between text-sm font-mono">
            <span className="text-green-700">
              显示 {(page - 1) * perPage + 1}-{Math.min(page * perPage, total)} / 共 {total} 条
            </span>
            <div className="flex items-center gap-2">
              <button
                onClick={() => setPage(p => Math.max(1, p - 1))}
                disabled={page <= 1}
                className="p-1 text-green-600 hover:text-green-400 disabled:opacity-30 disabled:cursor-not-allowed"
                title="上一页 [P]"
              >
                <ChevronLeft className="w-5 h-5" />
              </button>
              <span className="text-green-500 min-w-[60px] text-center">
                {page} / {totalPages}
              </span>
              <button
                onClick={() => setPage(p => Math.min(totalPages, p + 1))}
                disabled={page >= totalPages}
                className="p-1 text-green-600 hover:text-green-400 disabled:opacity-30 disabled:cursor-not-allowed"
                title="下一页 [N]"
              >
                <ChevronRight className="w-5 h-5" />
              </button>
            </div>
          </div>
        )}

        {/* Shortcut hint footer */}
        <div className="fixed bottom-4 right-4 text-[10px] text-green-800 font-mono bg-black/80 px-2 py-1 border border-green-900 z-50">
            FILTER: [/] | SEARCH: [S] | HELP: [SHIFT+?]
        </div>

        {/* Help Panel Modal */}
        {showHelp && (
          <div className="fixed inset-0 bg-black/80 z-[100] flex items-center justify-center p-4 overflow-y-auto" onClick={() => setShowHelp(false)}>
            <div className="bg-black border-2 border-green-500 p-4 sm:p-8 max-w-lg w-full mx-4 my-auto max-h-[90vh] flex flex-col shadow-[0_0_30px_rgba(34,197,94,0.4)]" onClick={e => e.stopPropagation()}>
              <div className="flex justify-between items-center mb-4 sm:mb-8 border-b border-green-800 pb-4 flex-shrink-0">
                <h2 className="text-green-400 font-bold text-xl sm:text-3xl tracking-widest">KEYBOARD_SHORTCUTS</h2>
                <button onClick={() => setShowHelp(false)} className="text-green-600 hover:text-green-400">
                  <X className="w-6 sm:w-8 h-6 sm:h-8" />
                </button>
              </div>

              <div className="space-y-4 sm:space-y-6 font-mono overflow-y-auto flex-1">
                <div className="grid grid-cols-2 gap-2 sm:gap-4 text-base sm:text-xl">
                  <div className="text-green-500 font-bold text-lg sm:text-2xl col-span-2">NAVIGATION</div>

                  <div className="text-green-600 pl-2 sm:pl-4">J / ↓</div>
                  <div className="text-green-300">下一条</div>

                  <div className="text-green-600 pl-2 sm:pl-4">K / ↑</div>
                  <div className="text-green-300">上一条</div>

                  <div className="text-green-600 pl-2 sm:pl-4">N</div>
                  <div className="text-green-300">下一页</div>

                  <div className="text-green-600 pl-2 sm:pl-4">P</div>
                  <div className="text-green-300">上一页</div>

                  <div className="text-green-500 font-bold text-lg sm:text-2xl mt-2 sm:mt-4 col-span-2">ACTIONS</div>

                  <div className="text-green-600 pl-2 sm:pl-4">L / Enter</div>
                  <div className="text-green-300">查看详情</div>

                  <div className="text-green-600 pl-2 sm:pl-4">/</div>
                  <div className="text-green-300">筛选</div>

                  <div className="text-green-600 pl-2 sm:pl-4">S</div>
                  <div className="text-green-300">全文搜索</div>

                  <div className="text-green-600 pl-2 sm:pl-4">R</div>
                  <div className="text-green-300">刷新</div>

                  <div className="text-green-600 pl-2 sm:pl-4">E</div>
                  <div className="text-green-300">导出</div>

                  <div className="text-green-600 pl-2 sm:pl-4">Escape</div>
                  <div className="text-green-300">关闭 / 取消</div>

                  <div className="text-green-600 pl-2 sm:pl-4">Shift + ?</div>
                  <div className="text-green-300">显示帮助</div>
                </div>
              </div>

              <div className="mt-4 sm:mt-8 pt-4 border-t border-green-900 text-center flex-shrink-0">
                <span className="text-green-700 text-sm sm:text-lg">Press ESC or click outside to close</span>
              </div>
            </div>
          </div>
        )}
    </div>
  );
};
