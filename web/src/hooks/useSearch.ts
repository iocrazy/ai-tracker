import { useState, useCallback, useRef, useEffect } from 'react';

interface UseSearchResult {
  query: string;
  setQuery: (q: string) => void;
  isActive: boolean;
  open: () => void;
  close: () => void;
  matchCount: number;
  currentIndex: number;
  next: () => void;
  prev: () => void;
  matchRefs: React.MutableRefObject<(HTMLElement | null)[]>;
  registerMatch: (index: number, el: HTMLElement | null) => void;
  resetMatches: (count: number) => void;
}

export function useSearch(): UseSearchResult {
  const [query, setQuery] = useState('');
  const [isActive, setIsActive] = useState(false);
  const [matchCount, setMatchCount] = useState(0);
  const [currentIndex, setCurrentIndex] = useState(0);
  const matchRefs = useRef<(HTMLElement | null)[]>([]);

  const open = useCallback(() => setIsActive(true), []);

  const close = useCallback(() => {
    setIsActive(false);
    setQuery('');
    setMatchCount(0);
    setCurrentIndex(0);
    matchRefs.current = [];
  }, []);

  const registerMatch = useCallback((index: number, el: HTMLElement | null) => {
    matchRefs.current[index] = el;
  }, []);

  const resetMatches = useCallback((count: number) => {
    setMatchCount(count);
    matchRefs.current = new Array(count).fill(null);
    if (count > 0) {
      setCurrentIndex(0);
    } else {
      setCurrentIndex(0);
    }
  }, []);

  const scrollToMatch = useCallback((index: number) => {
    const el = matchRefs.current[index];
    if (el) {
      el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, []);

  const next = useCallback(() => {
    if (matchCount === 0) return;
    const nextIdx = (currentIndex + 1) % matchCount;
    setCurrentIndex(nextIdx);
    scrollToMatch(nextIdx);
  }, [currentIndex, matchCount, scrollToMatch]);

  const prev = useCallback(() => {
    if (matchCount === 0) return;
    const prevIdx = (currentIndex - 1 + matchCount) % matchCount;
    setCurrentIndex(prevIdx);
    scrollToMatch(prevIdx);
  }, [currentIndex, matchCount, scrollToMatch]);

  // Auto-scroll to first match when query changes
  useEffect(() => {
    if (matchCount > 0) {
      scrollToMatch(0);
    }
  }, [query, matchCount, scrollToMatch]);

  return {
    query, setQuery, isActive, open, close,
    matchCount, currentIndex, next, prev,
    matchRefs, registerMatch, resetMatches,
  };
}
