# ProjectSettings Polish — Round 2

## Changes

### 1. DRY up add forms
Always render table shell (header + add row). Empty state message goes inside the table body area between header and add form. Eliminates ~50 lines of duplicated JSX.

### 2. Loading state
Add `loading` boolean. Show centered `LOADING...` with pulsing animation during initial fetch. Prevents flash of empty state.

### 3. Tab count badges
Show item counts: `VARIABLES (2)`, `SERVICES (3)`, `WORKTREES (1)`. Omit count when loading or when count is 0.

### 4. Keyboard UX
- Enter key in edit mode triggers save
- Smart Escape: cancel active edit first, close modal only if nothing is being edited
- Auto-focus first input in add form on tab switch

### 5. Row flash animation
After add/edit, briefly highlight the affected row (green flash, 0.5s fade). Maintains `flashId` state, cleared by timeout.

## File
`web/src/components/ProjectSettings.tsx` — single file change, no new files.
