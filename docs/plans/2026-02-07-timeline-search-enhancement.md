# Timeline Search Enhancement Design

## Overview

Enhance Timeline search with two capabilities:
1. **Timeline full-text search** - Search across conversation transcript content (not just summary)
2. **History Detail in-modal search** - Ctrl+F style search within the detail popup with highlight + jump

## 1. Timeline Full-Text Search

### UI
- Keep existing FILTER for quick client-side filtering (summary/session/action)
- Add a new search button next to FILTER that expands a full-text search bar
- Shortcut: `s` to focus full-text search
- Search results show keyword snippet highlights in Timeline cards

### Backend
- New endpoint: `GET /api/history/search?q=<keyword>&range=<range>&page=<page>&per_page=<per_page>`
- Searches across: `summary`, `completion_note`, and transcript file content
- Returns matching records with context snippets (surrounding text around match)
- Response adds `match_snippets: string[]` to each record

### Response Shape
```typescript
interface SearchResult {
  groups: HistoryGroup[]
  total: number
  query: string
  match_snippets: Record<number, string[]> // historyId -> snippets
}
```

## 2. History Detail In-Modal Search

### UI
- Search bar appears below tab row when activated
- Shows: input field + match count (e.g., "2/8") + up/down nav buttons
- Matched keywords highlighted with `bg-yellow-500/30 text-yellow-300`
- Auto-scrolls to current match on navigation

### Behavior
- Search scope: current active tab's text content
  - Messages tab: user + assistant message text
  - Summary tab: summary text
  - Tools tab: tool names + args + results
  - Commits tab: commit messages
- Highlight all matches, navigate between them with n/N
- Match counter updates in real-time as user types

### Implementation
- `useSearch` hook: manages query, matches array, currentIndex
- Text highlight component: wraps content, splits on query, renders `<mark>` for matches
- Each match gets a ref for scroll-into-view

## 3. Keyboard Shortcuts (Vim-style)

### Timeline List
| Key | Action |
|-----|--------|
| `/` | Focus FILTER (quick filter) |
| `s` | Focus full-text search |
| `j` / `k` | Move selection up/down |
| `l` / `Enter` | Open Detail |
| `n` / `p` | Next / previous page |
| `r` | Refresh |
| `e` | Export |
| `Escape` | Exit search input |

### History Detail Modal
| Key | Action |
|-----|--------|
| `/` | Open search bar |
| `n` | Jump to next match |
| `N` | Jump to previous match |
| `j` / `k` | Scroll content |
| `1-4` | Switch tabs |
| `q` / `Escape` | Layered: close search first, then close modal |

## 4. Files to Modify

### Frontend
- `web/src/components/TimelineView.tsx` - Add full-text search UI + shortcut
- `web/src/components/HistoryDetailModal.tsx` - Add in-modal search bar + highlight + navigation
- `web/src/hooks/useSearch.ts` - New hook for search state management
- `web/src/components/SearchHighlight.tsx` - New component for keyword highlighting
- `web/src/services/api.ts` - Add search API call

### Backend
- `src/rust/crates/tracker-server/src/main.rs` - Add `/api/history/search` endpoint
- `src/rust/crates/tracker-server/src/db.rs` - Add full-text search query (SQLite FTS or LIKE)
