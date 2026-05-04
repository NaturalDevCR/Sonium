# Sonium Web UI Redesign — Architecture

## Goals

1. **Works out-of-the-box** — Zero-config for basic use
2. **Progressive disclosure** — Simple by default, expert mode available
3. **Setup wizard** — Guides new users through first configuration
4. **Real-time sync visibility** — See multi-room sync health at a glance
5. **Mobile-first** — Most users will access from phones/tablets

## User Personas

| Persona | Needs |
|---------|-------|
| **Casual** | "I just want music in my house" — Volume, play/pause, group management |
| **Enthusiast** | "I want perfect sync" — Latency tuning, sync diagnostics, buffer settings |
| **Admin** | "I manage the server" — User management, config editing, system health |

## New Route Structure

```
/login                    → Login (unchanged)
/change-password          → Password change (unchanged)

/                         → **Dashboard** (new, replaces ControlView)
  ├─ Shows all groups with sync status
  ├─ Volume controls per group/client
  ├─ Stream selector per group
  ├─ Collapsible expert panels

/sync                     → **Sync Monitor** (new)
  ├─ Real-time clock drift visualization
  ├─ Group sync health per client
  ├─ Chrony setup guide if sync is poor
  ├─ Latency/buffer tuning

/matrix                   → Audio Matrix (unchanged)

/admin                    → Admin shell (redesigned)
  ├─ /admin/overview      → Server stats + quick actions
  ├─ /admin/streams       → Stream management
  ├─ /admin/clients       → Client management + sync status
  ├─ /admin/groups        → Group management
  ├─ /admin/system        → System info + logs + restart
  ├─ /admin/config        → Config editor
  ├─ /admin/users         → User management

/setup                    → **Setup Wizard** (new, accessible from dashboard)
  ├─ Step 1: Welcome + server status
  ├─ Step 2: Audio source configuration
  ├─ Step 3: Client discovery + grouping
  ├─ Step 4: Sync check + chrony recommendation
  ├─ Step 5: Done
```

## Component Architecture

### Layout

```
App.vue
├── AppShell (new)
│   ├── TopBar (logo, sync status indicator, user menu)
│   ├── NavDrawer (collapsible, context-aware)
│   └── MainContent
│       └── RouterView
└── Global overlays (toasts, modals, setup wizard)
```

### New Components

| Component | Purpose |
|-----------|---------|
| `SyncIndicator` | Global sync health icon (top bar) — green/yellow/red dot |
| `SyncPanel` | Expandable panel showing drift per client |
| `SetupWizard` | Multi-step onboarding flow |
| `ExpertToggle` | Switch that reveals advanced controls |
| `ClientCard` | Redesigned client tile with sync status |
| `GroupCard` | Redesigned group card with master sync status |
| `AudioVisualizer` | Mini spectrum/level meter in group header |
| `QuickActions` | Floating action buttons for common tasks |

### State Additions

```typescript
// stores/server.ts additions
const syncHealth = ref<Record<string, SyncHealth>>({}); // client_id → health
const serverTimezone = ref<string | null>(null);
const showExpertMode = ref(false); // persisted to localStorage

interface SyncHealth {
  status: 'good' | 'fair' | 'poor' | 'unknown';
  drift_ms: number;
  last_update_ms: number;
}
```

## Design Principles

1. **Color-coded sync status**
   - 🟢 Good: drift < 10ms
   - 🟡 Fair: drift 10-50ms
   - 🔴 Poor: drift > 50ms or no sync data
   - ⚪ Unknown: no sync data yet

2. **Contextual help**
   - Hover tooltips explain every control
   - "Why is sync poor?" expandable help sections
   - One-click copy of chrony install commands

3. **Progressive disclosure**
   - Default view: volume + mute + stream selector
   - Expert toggle reveals: latency, buffer, EQ, sync details
   - Admin tab only visible to admins

4. **Mobile optimization**
   - Bottom sheet modals instead of center dialogs
   - Swipe gestures between groups
   - Collapsible sections

## Data Flow

```
WebSocket Events → server store → Vue reactivity → UI components
                     ↓
              syncHealth computed from client.health + timestamps
                     ↓
              SyncIndicator (global) + SyncPanel (per group)
```

## Implementation Phases

### Phase 1: Foundation
- [ ] Create AppShell layout
- [ ] Redesign Dashboard view
- [ ] Add ExpertToggle component
- [ ] Persist expert mode to localStorage

### Phase 2: Sync Visibility
- [ ] SyncIndicator in top bar
- [ ] SyncPanel per group
- [ ] SyncMonitor page
- [ ] Contextual chrony help

### Phase 3: Setup Wizard
- [ ] SetupWizard component
- [ ] Step 1-5 flows
- [ ] Trigger from dashboard empty state

### Phase 4: Polish
- [ ] Mobile bottom nav redesign
- [ ] Animations and transitions
- [ ] Dark/light theme (if requested)

## API Changes Needed

1. **GET /api/sync-health** (new)
   - Returns global sync status + per-client drift
   - Or extend WebSocket events to include sync data

2. **GET /api/server/timezone** (new)
   - Returns configured timezone

3. **WebSocket: group_sync event** (new)
   - Broadcast when GroupSync messages are sent
   - Contains server timestamp for UI clock display

## Current → New Migration

| Old | New |
|-----|-----|
| ControlView.vue | Dashboard view (redesigned) |
| AdminView.vue | AppShell + Admin routes |
| HealthTab.vue | SyncMonitor page + SyncPanel |
| MatrixView.vue | Unchanged |

## Notes

- Keep existing API compatibility
- All new features are additive
- Expert mode is opt-in per user (localStorage)
- Setup wizard only shows on first visit (or manual trigger)
