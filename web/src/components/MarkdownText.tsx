import React from 'react';
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

/**
 * Renders markdown content with CRT-themed styling.
 * Optionally integrates SearchHighlight for keyword highlighting.
 */
export const MarkdownText: React.FC<MarkdownTextProps> = ({
  content,
  searchQuery,
  searchCurrentIndex = -1,
  searchStartMatchIndex = 0,
  onRegisterMatch,
  className = '',
  inline = false,
}) => {
  if (!content) return null;

  // Track match offset across multiple text nodes
  let matchOffset = searchStartMatchIndex;

  const highlightText = (text: string) => {
    if (!searchQuery || !onRegisterMatch) return text;
    const node = (
      <SearchHighlight
        text={text}
        query={searchQuery}
        currentIndex={searchCurrentIndex}
        startMatchIndex={matchOffset}
        onRegisterMatch={onRegisterMatch}
      />
    );
    // Advance offset by number of matches in this text
    const escaped = searchQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const regex = new RegExp(escaped, 'gi');
    const matches = text.match(regex);
    matchOffset += matches ? matches.length : 0;
    return node;
  };

  const components: Record<string, React.FC<any>> = {
    // Headings
    h1: ({ children }) => (
      <h1 className="text-green-200 text-lg font-bold mb-2 pb-1 border-b border-green-800/50">{children}</h1>
    ),
    h2: ({ children }) => (
      <h2 className="text-green-200 text-base font-bold mb-2 pb-1 border-b border-green-800/30">{children}</h2>
    ),
    h3: ({ children }) => (
      <h3 className="text-green-300 text-sm font-bold mb-1">{children}</h3>
    ),
    // Paragraph
    p: ({ children }) => (
      <p className="mb-2 last:mb-0 leading-relaxed">{children}</p>
    ),
    // Strong / Em
    strong: ({ children }) => (
      <strong className="text-green-100 font-bold">{children}</strong>
    ),
    em: ({ children }) => (
      <em className="text-green-400 italic">{children}</em>
    ),
    // Inline code
    code: ({ className: codeClassName, children, ...props }) => {
      // Block code (inside <pre>) vs inline code
      const isBlock = codeClassName?.startsWith('language-');
      if (isBlock) {
        return (
          <code className={`text-cyan-400 text-sm ${codeClassName || ''}`} {...props}>
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
    // Code block
    pre: ({ children }) => (
      <pre className="bg-black/60 border border-green-900/50 rounded p-3 mb-2 overflow-x-auto text-sm leading-relaxed">
        {children}
      </pre>
    ),
    // Lists
    ul: ({ children }) => (
      <ul className="list-disc list-inside mb-2 space-y-1 pl-2">{children}</ul>
    ),
    ol: ({ children }) => (
      <ol className="list-decimal list-inside mb-2 space-y-1 pl-2">{children}</ol>
    ),
    li: ({ children }) => (
      <li className="leading-relaxed">{children}</li>
    ),
    // Links
    a: ({ href, children }) => (
      <a href={href} className="text-cyan-400 underline hover:text-cyan-300" target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    ),
    // Horizontal rule
    hr: () => (
      <hr className="border-green-800/50 my-3" />
    ),
    // Blockquote
    blockquote: ({ children }) => (
      <blockquote className="border-l-2 border-green-700/50 pl-3 my-2 text-green-400/80 italic">
        {children}
      </blockquote>
    ),
    // Table
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

  // When search is active, wrap text nodes with SearchHighlight
  if (searchQuery && onRegisterMatch) {
    // Override text rendering in key elements
    const wrapWithHighlight = (Component: React.FC<any>): React.FC<any> => {
      return ({ children, ...props }) => {
        const processed = React.Children.map(children, (child) => {
          if (typeof child === 'string') {
            return highlightText(child);
          }
          return child;
        });
        return <Component {...props}>{processed}</Component>;
      };
    };

    // Apply highlight wrapping to text-containing elements
    for (const tag of ['p', 'li', 'td', 'th', 'h1', 'h2', 'h3', 'strong', 'em', 'blockquote']) {
      const original = components[tag];
      if (original) {
        components[tag] = wrapWithHighlight(original);
      }
    }

    // Special handling for inline code
    const originalCode = components.code!;
    components.code = ({ children, ...props }) => {
      const processed = React.Children.map(children, (child) => {
        if (typeof child === 'string') return highlightText(child);
        return child;
      });
      return originalCode({ children: processed, ...props });
    };
  }

  const wrapperClass = inline
    ? `markdown-text inline ${className}`
    : `markdown-text ${className}`;

  return (
    <div className={wrapperClass}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {content}
      </ReactMarkdown>
    </div>
  );
};
