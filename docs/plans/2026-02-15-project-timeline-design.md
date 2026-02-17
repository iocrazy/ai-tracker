# Project Timeline Sub-tab Design

## Goal

Replace the top-level TIMELINE tab with a per-project timeline sub-tab inside ProjectsView. Each project gets its own full-featured timeline (search, pagination, keyboard navigation, detail modal) scoped to that project's history. No global cross-project timeline.

## Architecture

### Component Extraction

Extract core rendering logic from `TimelineView.tsx` (649 lines) into a new `ProjectTimeline.tsx` component (~400 lines). Delete `TimelineView.tsx` afterward.

```
TimelineView.tsx (deleted)
    ↓ extract
ProjectTimeline.tsx (new)
    ├── Receives gitDir prop, auto-filters by project
    ├── Retains: search, time range, pagination, keyboard nav, detail modal
    ├── Removes: project filter bar (not needed inside project)
    └── Reuses: TimelineItem (already memo'd), HistoryDetailView
```

### Data Flow

```
ProjectsView (detail mode, timeline tab)
  └── <ProjectTimeline gitDir={selectedProject.git_dir} projectName={name} />
        ├── fetch: /api/projects/history?project={gitDir}&range=...&search=...&page=...
        ├── convert: HistoryEntry → TimelineEvent (reuse convertToTimelineEvent)
        ├── render: TimelineItem[] + pagination
        └── detail: HistoryDetailView modal (click item to open)
```

### Props

```typescript
interface ProjectTimelineProps {
  gitDir: string;        // project git_dir for API filtering
  projectName: string;   // display name
}
```

### Features Retained
- Time range selector (today/yesterday/7d/30d/all)
- Search (local filter + server-side full-text search)
- Pagination (50/page, prev/next)
- Keyboard navigation (j/k/n/p/l/Enter/s/r)
- Detail modal (HistoryDetailView)
- JSON export

### Features Removed
- Project filter bar (already scoped to project)
- "ALL" global mode (no global view)

## Tab Structure Changes

### ProjectsView detail tabs
```
Before: overview | env-vars | worktrees | statistics
After:  overview | timeline | env-vars | worktrees | statistics
```

### Top-level AppTab
```
Before: WORKSTATIONS | PROJECTS | TIMELINE | ANALYTICS | CONSOLE | SETTINGS
After:  WORKSTATIONS | PROJECTS | ANALYTICS | CONSOLE | SETTINGS
```

## Files

| File | Action |
|------|--------|
| `web/src/components/ProjectTimeline.tsx` | Create — extracted from TimelineView |
| `web/src/components/ProjectsView.tsx` | Modify — add timeline sub-tab |
| `web/src/components/TimelineView.tsx` | Delete |
| `web/src/App.tsx` | Modify — remove TIMELINE case, update tab arrays |
| `web/src/types.ts` | Modify — remove 'TIMELINE' from AppTab |
| `web/src/components/CommandPalette.tsx` | Modify — remove "Go to Timeline" nav item |
| `web/src/test/CommandPalette.test.tsx` | Modify — update test assertions |
