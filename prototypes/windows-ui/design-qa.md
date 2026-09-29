# Windows HTML prototype design QA

- Date: 2026-09-29
- Source visual truth: `../../docs/ui-prototypes/windows/status-selected-light-v1.png`, `../../docs/ui-spec-v1.md`, and the user's targeted feedback in `qa-reference-icon.png`, `qa-reference-status.png`, and `qa-reference-files.png`. The earlier selected image establishes hierarchy and palette; the annotation specifically asks to refine the logo, remove the tiny captions above page titles, and move content upward.
- Browser implementation: `http://127.0.0.1:4173/`, status screen, light theme.
- Browser capture: `status-capture.png` (880 × 880 px, device scale 1; centered app window approximately 830 × 620 CSS px).
- Source: 1516 × 1038 px, cropped to the 1473 × 996 px window and proportionally normalized to 830 px wide. The original has a different aspect ratio, so vertical positions are compared structurally rather than pixel for pixel.
- Full-view comparison: `status-comparison.png`; title, navigation and hero focused comparison: `status-comparison-focus.png`. The exact annotated icon and title areas were compared side by side in `annotation-comparison.png`. These artifacts were opened and inspected together.

## Findings

No remaining P0/P1/P2 problems in the desktop status screen. The primary state, connected phone, current target, LAN address and pairing action remain visible without scrolling. The prototype intentionally changes the source's larger navigation selection and plain status heading to an inset selection and quiet status surface. It adds file transfer to the navigation because that feature now exists; these are deliberate product updates, not fidelity defects.

The first browser capture exposed a vertical scrollbar that clipped the bottom action on the status and settings screens (P2). Content padding, row heights and section rhythm were reduced; the later status and settings captures show all primary controls without scrolling.

The user's subsequent annotation found that the Windows-generated ICO looked rough at titlebar size and the tiny captions above the titles were unwanted (P2). The HTML prototype now renders a clean library pen mark with a teal dot in the same black-square motif, hides the captions across status, files, phones and settings, and reduces top padding by 8 px. Pairing's functional return action is moved beside its title rather than above it. Final browser captures of all five pages show the intended title hierarchy and no newly introduced scrollbar.

## Fidelity surfaces

- Typography: Windows system-friendly Segoe UI / Microsoft YaHei UI fallback; title and object names have clear weight. Auxiliary copy is intentionally smaller than the old raster mock; a few secondary labels could be enlarged after user review (P3).
- Spacing: fixed sidebar, aligned content column, consistent 35 px controls and 55 px status rows. No accidental clipping at the tested desktop viewport.
- Color: white and cool gray surfaces; the product's `#00BAB8` for its logo dot/selection/activity accents, darker teal for text and primary-button contrast. Offline/waiting state remains separately labeled.
- Assets: the titlebar mark is composed from the same line-icon library as the navigation, following the existing black-square, white-pen and teal-dot concept. This prototype change does **not** alter the installed Windows icon yet. The QR code is real rendered QR artwork carrying a clearly labeled **demo-only** value, never a pairing credential.
- Copy: specific, task-oriented status labels; simulation controls and demo data are explicitly identified as prototype-only.

## Interaction evidence

In the browser, navigation to all five views, QR binding simulation, paired-device count update, the unbind confirmation and cancel path, received-files tab, settings name edit/save, startup toggle and update-check loading state were exercised. File-picker upload and operating-system folder opening were not exercised because they would access local files; transfer progress is implemented as a local demonstration state. Production syncing and file transfer are intentionally outside this prototype.

## Follow-up polish

Confirm the revised titlebar mark and higher page titles with the user. Do not port the design to Win32 before that decision.

final result: passed
