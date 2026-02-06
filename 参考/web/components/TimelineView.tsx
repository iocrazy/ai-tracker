import React, { useState, useEffect, useRef, useMemo } from 'react';
import { TimelineEvent } from '../types';
import { ChevronRight, Search } from 'lucide-react';

interface TimelineViewProps {
  events: TimelineEvent[];
  onViewDetails: (event: TimelineEvent) => void;
  isActive: boolean;
}

export const TimelineView: React.FC<TimelineViewProps> = ({ events, onViewDetails, isActive }) => {
  const [search, setSearch] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  
  const searchInputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLDivElement | null)[]>([]);

  // Filter events based on search query
  const filteredEvents = useMemo(() => {
    return events.filter(e => 
      e.description.toLowerCase().includes(search.toLowerCase()) || 
      e.user.toLowerCase().includes(search.toLowerCase()) ||
      e.action.toLowerCase().includes(search.toLowerCase())
    );
  }, [events, search]);

  // Reset selection when filter changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [search]);

  // Handle Keyboard Shortcuts
  useEffect(() => {
    if (!isActive) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Ignore navigation keys if typing in search
      if (document.activeElement === searchInputRef.current && e.key !== 'Escape' && e.key !== 'Enter') {
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
        case 'l':
        case 'Enter':
          e.preventDefault();
          if (filteredEvents[selectedIndex]) {
            onViewDetails(filteredEvents[selectedIndex]);
            // If in search, blur to allow navigation again after return
            searchInputRef.current?.blur();
          }
          break;
        case 'Escape':
          if (document.activeElement === searchInputRef.current) {
            searchInputRef.current?.blur();
          }
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isActive, filteredEvents, selectedIndex, onViewDetails]);

  // Auto-scroll to selected item
  useEffect(() => {
    const el = itemRefs.current[selectedIndex];
    if (el) {
      el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, [selectedIndex]);

  return (
    <div className="retro-border bg-black/40 p-1 flex flex-col relative min-h-[500px]">
        {/* Header - Sticky */}
        <div className="p-6 pb-4 border-b border-green-900/30 flex justify-between items-center sticky top-0 bg-black/95 z-40 backdrop-blur-sm">
            <span className="bg-green-500 text-black text-2xl font-bold px-3 py-1 font-['Share_Tech_Mono'] uppercase tracking-widest shadow-[0_0_10px_rgba(34,197,94,0.6)]">
                TODAY'S LOGS
            </span>
            
            {/* Search Box */}
            <div className="flex items-center gap-2 group relative">
                <Search className="w-5 h-5 text-green-700" />
                <input 
                    ref={searchInputRef}
                    type="text" 
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    placeholder="FILTER HISTORY [/]"
                    className="bg-black border-b border-green-800 text-green-400 font-mono focus:outline-none focus:border-green-400 placeholder-green-900 w-64 py-1"
                />
                <div className="absolute right-0 top-0 text-[10px] text-green-900 bg-green-900/20 px-1 border border-green-900/50 opacity-50">
                    ESC to blur
                </div>
            </div>
        </div>

        {/* Full Page List Container */}
        <div ref={containerRef} className="flex-grow p-6 pl-10">
            <div className="relative border-l-2 border-green-800/30 pl-8 space-y-8 pb-10">
                {filteredEvents.length === 0 ? (
                    <div className="text-green-800 font-mono italic p-4">NO_RECORDS_FOUND</div>
                ) : (
                    filteredEvents.map((event, index) => {
                        const isSelected = index === selectedIndex;
                        return (
                            <div 
                                key={event.id}
                                ref={(el) => itemRefs.current[index] = el}
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
                                        <div className="flex-grow">
                                            <div className="flex items-center gap-4 mb-2">
                                                <span className={`font-bold text-xl tracking-wider ${isSelected ? 'text-white' : 'text-green-300'}`}>
                                                    {event.user}
                                                </span>
                                                <div className={`h-px w-12 transition-colors ${isSelected ? 'bg-green-400' : 'bg-green-800'}`}></div>
                                                <span className={`font-bold tracking-widest uppercase text-sm border px-2 py-0.5 transition-all
                                                    ${isSelected 
                                                        ? 'text-green-900 bg-green-400 border-green-400' 
                                                        : 'text-cyan-400 border-cyan-900/50 bg-cyan-900/10'
                                                    }
                                                `}>
                                                    {event.action}
                                                </span>
                                            </div>
                                            
                                            <div className={`text-lg font-sans tracking-wide mb-2 leading-relaxed max-w-3xl transition-colors
                                                ${isSelected ? 'text-green-100' : 'text-green-500/80'}
                                            `}>
                                                {event.description}
                                            </div>

                                            <div className={`flex items-center gap-2 text-sm font-mono tracking-widest transition-colors
                                                ${isSelected ? 'text-green-300' : 'text-cyan-700'}
                                            `}>
                                                <span>VIEW_DETAILS [L]</span>
                                                <ChevronRight className="w-4 h-4" />
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
        
        {/* Shortcut hint footer */}
        <div className="fixed bottom-4 right-4 text-[10px] text-green-800 font-mono bg-black/80 px-2 py-1 border border-green-900 z-50">
            NAVIGATE: [J/K] • SELECT: [L] • SEARCH: [/]
        </div>
    </div>
  );
};