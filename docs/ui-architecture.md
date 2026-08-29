# UI Architecture

The two clients use the same ownership rule: runtime/session state lives below the UI, while screens only render state and dispatch user actions.

## Android

`FlowTypeApplication` is a small Android lifecycle composition root. It creates `FlowTypeController`, which is the application-level facade and single source of truth for binding, connection, the active per-computer input session and image transfer. Parked sessions are encrypted and stored independently by computer; only the selected computer is attached to the live WSS queue. `MainActivity` is a lifecycle and navigation coordinator. It owns activity-result contracts, keyboard/window policy and pairing dialogs.

Long-running or specialized work is delegated below the facade:

- `StorageDispatcher` serializes draft and history I/O away from the main thread.
- `DraftRepository` keeps an in-memory authoritative draft snapshot and writes encrypted state through the storage dispatcher.
- `ControlClientPool` owns the passive control sockets for non-selected computers.
- `AutoSelectionCoordinator` owns probe generations and ignores stale probe results.
- `ComputerChooser` renders the shared computer-selection model for both the activity and overlay surfaces.

Page rendering is split into small classes under `android/app/src/main/java/app/flowtype/ui/`:

- `HistoryScreen` renders history and detail views.
- `ComputersScreen` renders the binding list and delegates mutations.
- `SettingsScreen` binds persistent settings and delegates overlay permission flow.
- `ImageScreen` owns preview/prepare work and keeps bitmap processing off the main thread.
- `Screen` is the navigation state; it is not a second copy of session state.

The renderers consume `UiState` and dispatch controller commands. They do not access SQLite, encrypted draft storage, connection clients or session models directly. This prevents a page transition, floating window, or activity recreation from creating competing synchronization state.

## Windows

The Win32 message loop and `UiContext` remain the lifecycle boundary. UI-only concerns are separated into:

- `ui_commands.rs`: command IDs and conversion from `WM_COMMAND` IDs to typed actions.
- `ui_theme.rs`: palette constants used by every page.
- `ui_paint.rs`: GDI fonts, text, shapes, icons, menus and UTF-16 helpers.
- `ui_layout.rs`: DPI-independent constraints and page geometry.
- `ui_direct2d.rs`: Direct2D/DirectWrite rendering backed only by Windows system DLLs.
- `ui_pages.rs`: page composition, native control creation and responsive positioning.
- `ui_actions.rs`: state-driven refreshes and typed UI command handlers.
- `ui_tray.rs`: notification-area icon and menu lifecycle.
- `ui_render.rs`: owner-drawn controls and page-surface painting.
- `ui.rs`: `UiContext`, window registration, message dispatch and resource lifetime.

Network, pairing, persistence and injector code remain outside the UI modules. `network_server.rs` owns WSS connection handling, `pairing_store.rs` owns transactional paired-phone state, `atomic_file.rs` owns durable replacement, and `injector_dispatcher.rs` owns serialized blocking IPC. The UI reads `AppState::snapshot()` and posts commands back to state methods; it does not mutate protocol state directly.

The Windows management window uses a hybrid rendering boundary. Direct2D draws page surfaces, separators, indicators and decorative icons; native child controls remain responsible for editable fields, commands and meaningful text so keyboard behavior, Unicode font fallback and UI Automation semantics are preserved. Run `flowtype.exe --ui-preview --show` for visual review without opening the LAN listener, or `flowtype.exe --ui-preview-pairing` to open the pairing page directly.

## Refactoring Rules

1. Keep one authoritative active runtime/session state per process, with at most one encrypted parked session per paired computer.
2. Keep network and injector behavior independent from screen rendering.
3. Prefer typed commands and callbacks at UI boundaries.
4. Keep platform-unsafe code localized to platform UI modules.
5. Make a behavior-preserving split before changing product behavior, then add focused tests for the new boundary.
