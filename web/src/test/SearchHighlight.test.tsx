import { render, screen } from '@testing-library/react';
import { SearchHighlight, countMatches } from '../components/SearchHighlight';

describe('countMatches', () => {
  it('counts case-insensitive matches', () => {
    expect(countMatches('Hello hello HELLO', 'hello')).toBe(3);
  });

  it('returns 0 for no match', () => {
    expect(countMatches('Hello world', 'xyz')).toBe(0);
  });

  it('returns 0 for empty query', () => {
    expect(countMatches('Hello', '')).toBe(0);
  });

  it('returns 0 for empty text', () => {
    expect(countMatches('', 'hello')).toBe(0);
  });

  it('handles regex special chars in query', () => {
    expect(countMatches('a.b a.b', 'a.b')).toBe(2);
    expect(countMatches('foo(bar)', '(bar)')).toBe(1);
  });
});

describe('SearchHighlight', () => {
  const noop = () => {};

  it('renders plain text when no query', () => {
    render(
      <SearchHighlight text="Hello world" query="" currentIndex={-1} startMatchIndex={0} onRegisterMatch={noop} />
    );
    expect(screen.getByText('Hello world')).toBeInTheDocument();
  });

  it('highlights matching text', () => {
    render(
      <SearchHighlight text="Hello world" query="world" currentIndex={-1} startMatchIndex={0} onRegisterMatch={noop} />
    );
    const highlight = screen.getByText('world');
    expect(highlight.tagName).toBe('SPAN');
    expect(highlight.className).toContain('bg-yellow');
  });

  it('highlights multiple occurrences', () => {
    render(
      <SearchHighlight text="foo bar foo" query="foo" currentIndex={-1} startMatchIndex={0} onRegisterMatch={noop} />
    );
    const matches = screen.getAllByText('foo');
    expect(matches).toHaveLength(2);
  });

  it('calls onRegisterMatch for each match', () => {
    const registerMock = vi.fn();
    render(
      <SearchHighlight text="a b a" query="a" currentIndex={-1} startMatchIndex={5} onRegisterMatch={registerMock} />
    );
    // Should register match at global index 5 and 6
    expect(registerMock).toHaveBeenCalledWith(5, expect.anything());
    expect(registerMock).toHaveBeenCalledWith(6, expect.anything());
  });

  it('applies current highlight style to currentIndex match', () => {
    render(
      <SearchHighlight text="a b a" query="a" currentIndex={1} startMatchIndex={0} onRegisterMatch={noop} />
    );
    const matches = screen.getAllByText('a');
    // Second match (index 1) should have current style
    expect(matches[1].className).toContain('bg-yellow-400');
    // First match (index 0) should have normal style
    expect(matches[0].className).toContain('bg-yellow-600');
  });
});
