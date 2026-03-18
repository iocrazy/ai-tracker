import React, { useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { SearchHighlight } from './SearchHighlight';

interface MarkdownTextProps {
  content: string;
  searchQuery?: string;
  searchCurrentIndex?: number;
  searchStartMatchIndex?: number;
  onRegisterMatch?: (globalIndex: number, el: HTMLElement | null) => void;
  /** Extra class names for the wrapper */
  className?: string;
  /** Render inline (no block-level wrapper) for short single-line text */
  inline?: boolean;
}

// Module-level constant — never recreated between renders
const MARKDOWN_COMPONENTS: Record<string, React.FC<any>> = {
  h1: ({ children }) => (
    <h1 className="text-green-200 text-lg font-bold mb-2 pb-1 border-b border-green-800/50">{children}</h1>
  ),
  h2: ({ children }) => (
    <h2 className="text-green-200 text-base font-bold mb-2 pb-1 border-b border-green-800/30">{children}</h2>
  ),
  h3: ({ children }) => (
    <h3 className="text-green-300 text-sm font-bold mb-1">{children}</h3>
  ),
  p: ({ children }) => (
    <p className="mb-2 last:mb-0 leading-relaxed">{children}</p>
  ),
  strong: ({ children }) => (
    <strong className="text-green-100 font-bold">{children}</strong>
  ),
  em: ({ children }) => (
    <em className="text-green-400 italic">{children}</em>
  ),
  code: ({ className: codeClassName, children, ...props }) => {
    const isBlock = codeClassName?.startsWith('language-');
    if (isBlock) {
      return (
        <code className={`text-cyan-400 text-sm font-mono ${codeClassName || ''}`} {...props}>
          {children}
        </code>
      );
    }
    return (
      <code className="bg-black/50 text-cyan-400 px-1.5 py-0.5 rounded text-sm font-mono" {...props}>
        {children}
      </code>
    );
  },
  pre: ({ children }) => (
    <pre className="bg-black/60 border border-green-900/50 rounded p-3 mb-2 overflow-x-hidden whitespace-pre-wrap break-all text-sm leading-tight font-mono">
      {children}
    </pre>
  ),
  ul: ({ children }) => (
    <ul className="list-disc list-inside mb-2 space-y-1 pl-2">{children}</ul>
  ),
  ol: ({ children }) => (
    <ol className="list-decimal list-inside mb-2 space-y-1 pl-2">{children}</ol>
  ),
  li: ({ children }) => (
    <li className="leading-relaxed">{children}</li>
  ),
  a: ({ href, children }) => (
    <a href={href} className="text-cyan-400 underline hover:text-cyan-300" target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  ),
  hr: () => (
    <hr className="border-green-800/50 my-3" />
  ),
  blockquote: ({ children }) => (
    <blockquote className="border-l-2 border-green-700/50 pl-3 my-2 text-green-400/80 italic">
      {children}
    </blockquote>
  ),
  table: ({ children }) => (
    <div className="overflow-x-auto mb-2">
      <table className="w-full text-sm border-collapse">{children}</table>
    </div>
  ),
  thead: ({ children }) => (
    <thead className="border-b border-green-800">{children}</thead>
  ),
  th: ({ children }) => (
    <th className="text-left text-green-300 font-bold px-2 py-1">{children}</th>
  ),
  td: ({ children }) => (
    <td className="text-green-400 px-2 py-1 border-t border-green-900/30">{children}</td>
  ),
};

const REMARK_PLUGINS = [remarkGfm];

/**
 * Renders markdown content with CRT-themed styling.
 * Optionally integrates SearchHighlight for keyword highlighting.
 */
export const MarkdownText: React.FC<MarkdownTextProps> = React.memo(({
  content,
  searchQuery,
  searchCurrentIndex = -1,
  searchStartMatchIndex = 0,
  onRegisterMatch,
  className = '',
  inline = false,
}) => {
  if (!content) return null;

  // Build search-aware components only when search is active.
  // matchOffset is mutable per-render (tracks across text nodes), so this
  // can't be a pure useMemo — it's intentionally rebuilt each render when searching.
  const components = useMemo(() => {
    if (!searchQuery) return MARKDOWN_COMPONENTS;

    // Clone base components and wrap text-containing elements with search highlight
    let matchOffset = searchStartMatchIndex;

    const highlightText = (text: string) => {
      const node = (
        <SearchHighlight
          text={text}
          query={searchQuery}
          currentIndex={searchCurrentIndex}
          startMatchIndex={matchOffset}
          onRegisterMatch={onRegisterMatch}
        />
      );
      const escaped = searchQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const regex = new RegExp(escaped, 'gi');
      const matches = text.match(regex);
      matchOffset += matches ? matches.length : 0;
      return node;
    };

    const wrapWithHighlight = (Component: React.FC<any>): React.FC<any> => {
      return ({ children, ...props }) => {
        const processed = React.Children.map(children, (child) => {
          if (typeof child === 'string') return highlightText(child);
          return child;
        });
        return <Component {...props}>{processed}</Component>;
      };
    };

    const wrapped: Record<string, React.FC<any>> = { ...MARKDOWN_COMPONENTS };

    for (const tag of ['p', 'li', 'td', 'th', 'h1', 'h2', 'h3', 'strong', 'em', 'blockquote']) {
      const original = wrapped[tag];
      if (original) {
        wrapped[tag] = wrapWithHighlight(original);
      }
    }

    // Special handling for inline code
    const originalCode = wrapped.code!;
    wrapped.code = ({ children, ...props }) => {
      const processed = React.Children.map(children, (child) => {
        if (typeof child === 'string') return highlightText(child);
        return child;
      });
      return originalCode({ children: processed, ...props });
    };

    return wrapped;
    // Note: matchOffset is mutable state that changes during render, so we
    // intentionally don't try to memoize when search is active. The key win
    // is the non-search path (vast majority of renders) using the stable constant.
  }, [searchQuery, searchCurrentIndex, searchStartMatchIndex, onRegisterMatch]);

  const wrapperClass = inline
    ? `markdown-text inline ${className}`
    : `markdown-text ${className}`;

  return (
    <div className={wrapperClass}>
      <ReactMarkdown remarkPlugins={REMARK_PLUGINS} components={components}>
        {content}
      </ReactMarkdown>
    </div>
  );
});

MarkdownText.displayName = 'MarkdownText';
