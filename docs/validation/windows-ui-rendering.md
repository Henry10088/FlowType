# Windows UI rendering validation

Date: 2026-08-27

## Scope

- Added a DPI-independent constraint layout for the status, paired-phones, settings and pairing pages.
- Added hybrid Direct2D/DirectWrite rendering for surfaces, separators, indicators and decorative icons.
- Retained native text and command controls for Unicode fallback, keyboard behavior and UI Automation.
- Replaced dotted owner-draw focus rectangles with the existing teal focus treatment.
- Added `--ui-preview` to render real local state without opening the LAN listener.

## Results

- Chinese and English were visually checked on all migrated pages at the same 760 x 600 window size.
- `Offline · Connected previously` fits without colliding with the `Unpair` action.
- The English settings header, update actions and bottom note fit without clipping.
- The English pairing instructions wrap within a reserved two-line area without colliding with the scan instruction.
- UI Automation exposes the page title, phone name, connection status and every command.
- The preview process had zero listening sockets.
- Workspace tests: 40 passed; 2 interactive input tests skipped by design.
- Clippy: zero warnings.

## Size

| Artifact | Previous | Current | Increase |
| --- | ---: | ---: | ---: |
| `flowtype.exe` | 2,287,616 bytes | 2,298,368 bytes | 10,752 bytes (10.5 KiB, 0.47%) |

The current machine does not have the Inno Setup compiler installed, so the installer was not rebuilt for an exact compressed-size comparison. Direct2D and DirectWrite are Windows system DLLs and are not packaged with FlowType.
