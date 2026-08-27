# Windows UI rendering validation

Date: 2026-08-27

## Scope

- Added a DPI-independent constraint layout for the paired-phones page.
- Added hybrid Direct2D/DirectWrite rendering for surfaces, separators, indicators and decorative icons.
- Retained native text and command controls for Unicode fallback, keyboard behavior and UI Automation.
- Replaced dotted owner-draw focus rectangles with the existing teal focus treatment.
- Added `--ui-preview` to render real local state without opening the LAN listener.

## Results

- Chinese and English were visually checked at the same 760 x 600 window size.
- `Offline · Connected previously` fits without colliding with the `Unpair` action.
- UI Automation exposes the page title, phone name, connection status and every command.
- The preview process had zero listening sockets.
- Workspace tests: 37 passed; 2 interactive input tests skipped by design.
- Clippy: zero warnings.

## Size

| Artifact | Previous | Current | Increase |
| --- | ---: | ---: | ---: |
| `flowtype.exe` | 2,287,616 bytes | 2,292,736 bytes | 5,120 bytes (5.0 KiB, 0.22%) |

The current machine does not have the Inno Setup compiler installed, so the installer was not rebuilt for an exact compressed-size comparison. Direct2D and DirectWrite are Windows system DLLs and are not packaged with FlowType.
