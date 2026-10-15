# Prototype Instructions

This is a visual and interaction prototype for FlowType's Windows app. Keep the established light theme, restrained teal accent, and existing page hierarchy. It is separate from Win32 and must not connect to devices or change transfer or sync protocols.

User-approved refinement: page titles have no small eyebrow/caption above them and content starts higher in the window. The titlebar mark uses the theme teal background with a white pen and must render cleanly at small sizes.

Run the local server yourself and open the preview in the browser available to this environment. Do not give the user server-start instructions when you can run it.

Before making substantial visual changes, use the Product Design plugin's `get-context` skill when the visual source is unclear or no longer matches the current goal. When the user gives durable prototype-specific design feedback, preferences, or decisions, record them in `AGENTS.md`.

When implementing from a selected generated mock, treat that image as the source of truth for layout, component anatomy, density, spacing, color, typography, visible content, and hierarchy.

Build app UI in `src/`. Keep `.openai/hosting.json`, `worker/index.js`, `scripts/prepare-sites-build.mjs`, and `tests/sites-worker.test.mjs` intact so the same local prototype can be handed to Sites. Before a Sites handoff, run `npm run build` and `npm run test:sites`; the build must leave `dist/client/index.html`, `dist/server/index.js`, and `dist/.openai/hosting.json`.
