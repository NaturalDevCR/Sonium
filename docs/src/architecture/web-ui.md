# Web UI

Sonium ships a built-in web UI served directly from the control port (`1711`).
No separate process, no separate install. The root view is the day-to-day
control surface; `/admin` is the administrator dashboard.

## Technology stack

| Layer | Choice | Reason |
|---|---|---|
| Framework | Vue 3 (Composition API) | Lightweight, reactive, no build-time SSR needed |
| State | Pinia | Official Vue state library; simpler than Vuex |
| Build | Vite 5 | Fast HMR in dev; single-bundle output for embedding |
| Types | TypeScript | Shared type definitions with `lib/api.ts` |
| Serving | Rust (`rust-embed`) | SPA assets baked into the binary; zero runtime dependencies |

## Project layout

```
web/
  index.html          Entry HTML
  vite.config.ts      Vite + proxy config (dev → localhost:1711)
  tsconfig.json
  src/
    main.ts           Mount Vue app, bootstrap store
    App.vue           Root router shell
    lib/
      api.ts          REST client + WebSocket + TypeScript types
    stores/
      server.ts       Pinia store — mirrors ServerState from the Rust backend
    components/
      StreamBadge.vue     Colored status pill
    views/
      ControlView.vue     Main control surface
      AdminView.vue       Admin shell
      admin/              Dashboard, streams, groups, clients, health, system, config, users
```

## State architecture

The Pinia store (`useServerStore`) is the single source of truth for all
server-side state in the browser:

```
REST bootstrap (loadAll)
       │
       ▼
┌──────────────────────┐
│  useServerStore      │   clients[], groups[], streams[], uptime
│  (Pinia)             │   connectedClients (computed)
└──────────────────────┘   clientsById, streamsById (computed)
       ▲
       │ applyEvent(event)
       │
WebSocket (/api/events)
```

On startup `main.ts` calls `loadAll()` (REST snapshot) then
`startLiveUpdates()` (WS subscription).  Every mutation that happens on the
server is pushed as a typed JSON event and applied in-place — the UI never
polls.

Auto-reconnect: if the WebSocket closes, the store waits 3 seconds, re-fetches
the full REST snapshot, and opens a new WebSocket connection.

## Development workflow

```bash
cd web
npm install
npm run dev        # Vite dev server on :5173, proxies /api → :1711
```

The Vite proxy config (`vite.config.ts`) forwards all `/api` requests
(including the WebSocket upgrade) to a running `sonium-server` on port `1711`.

## Production build

```bash
cd web
npm run build      # outputs to web/dist/
```

The Rust control server embeds the `web/dist/` directory at compile time using
`rust-embed` (planned Fase 7).  Any request that does not match `/api/*` is
served as the SPA `index.html` (catch-all SPA routing).

## Features

- **Control view** — group cards, stream selector, client volume/mute controls,
  group master volume, and role-aware controls.
- **Admin dashboard** — live count of connected clients, groups, streams, and uptime.
- **Streams tab** — source templates, URI builder, meta-stream chains, buffer and
  `chunk_ms` controls, plus a restart prompt after saving config changes.
- **Health tab** — client health/observability and filterable server logs.
- **System tab** — OS/audio stack info, dependency checks, package actions,
  time-window log viewer, and supervised restart request.
- **Config tab** — raw TOML editor with validation and restart button.
- **Users tab** — create/edit/delete users and roles.
- **Dark theme** — CSS custom properties, no external CSS framework

Admins land on `/admin` after login. Non-admin users land on the control view
unless they were following a specific redirect.
