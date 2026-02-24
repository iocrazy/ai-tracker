import React, { useEffect, useRef } from 'react';

interface SearchHighlightProps {
  text: string;
  query: string;
  currentIndex: number;
  startMatchIndex: number;
  onRegisterMatch?: (globalIndex: number, el: HTMLElement | null) => void;
}

/**
 * Renders text with keyword highlights.
 * Each match is assigned a global index (startMatchIndex + local offset)
 * so the parent can track and navigate all matches across multiple blocks.
 */
export const SearchHighlight: React.FC<SearchHighlightProps> = ({
  text,
  query,
  currentIndex,
  startMatchIndex,
  onRegisterMatch,
}) => {
  if (!query || !text) {
    return <>{text}</>;
  }

  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const regex = new RegExp(`(${escaped})`, 'gi');
  const parts = text.split(regex);

  let localMatchIdx = 0;

  return (
    <>
      {parts.map((part, i) => {
        if (regex.test(part)) {
          regex.lastIndex = 0; // reset after test
          const globalIdx = startMatchIndex + localMatchIdx;
          const isCurrent = globalIdx === currentIndex;
          localMatchIdx++;

          return (
            <HighlightMark
              key={i}
              globalIndex={globalIdx}
              isCurrent={isCurrent}
              onRegister={onRegisterMatch}
            >
              {part}
            </HighlightMark>
          );
        }
        return <React.Fragment key={i}>{part}</React.Fragment>;
      })}
    </>
  );
};

interface HighlightMarkProps {
  globalIndex: number;
  isCurrent: boolean;
  onRegister?: (index: number, el: HTMLElement | null) => void;
  children: React.ReactNode;
}

const HighlightMark: React.FC<HighlightMarkProps> = ({
  globalIndex,
  isCurrent,
  onRegister,
  children,
}) => {
  const ref = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!onRegister) return;
    onRegister(globalIndex, ref.current);
    return () => onRegister(globalIndex, null);
  }, [globalIndex, onRegister]);

  return (
    <span
      ref={onRegister ? ref : undefined}
      className={
        isCurrent
          ? 'bg-yellow-400/50 text-yellow-200 rounded px-0.5'
          : 'bg-yellow-600/30 text-yellow-300 rounded px-0.5'
      }
    >
      {children}
    </span>
  );
};

/**
 * Count the number of matches of query in text.
 */
export function countMatches(text: string, query: string): number {
  if (!query || !text) return 0;
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const regex = new RegExp(escaped, 'gi');
  const matches = text.match(regex);
  return matches ? matches.length : 0;
}
