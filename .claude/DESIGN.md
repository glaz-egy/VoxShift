## Overview

VoxShift is a native Slint desktop tray utility (820×560 recommended, 720×500
minimum) — not a web surface. There are no hover-driven marketing interactions,
no breakpoints, no oversized display type. Every token here exists to serve one
job: let a user glance at the window (or the tray icon) and instantly read two
things — "what is VRChat/Discord doing" and "which way is the link pointing" —
without ever depending on color alone (§7.2 of 設計書.md).

**Key characteristics:**
- Deep navy-to-black vertical gradient canvas — quiet, non-animated, recedes
  behind the two service cards.
- Exactly two "service" accents: VRChat = cyan/teal, Discord = blue/purple.
  These never appear except attached to VRChat or Discord content.
- Four "state" accents — green/amber/red/gray — always paired with an icon
  and a text label (never a bare color chip).
- Neutral (non-hued) chrome: sidebar selection and focus rings use bright/dark
  neutrals, not a third invented brand color, so state colors stay meaningful.
- Compact spacing (4px base unit) and small type (11–18px) sized for a
  desktop utility window, not a marketing page.
- Motion is a signal, not decoration: 200ms fade / 260ms pulse ONLY on state
  change, a transient connecting-ring ONLY while connecting, and a
  "reduced motion" flag that collapses all of the above to 0ms.

## Colors

### Service Accents
- **VRChat Teal** (`{colors.vrchat}` — dark `#35D8C6` / light `#12A190`):
  VRChat card header, icon tint, card border gradient start. Never used for
  generic chrome.
- **Discord Blurple** (`{colors.discord}` — dark `#5865F2` / light `#4752C4`):
  Discord card header, icon tint, card border gradient start.

### State Accents (always icon + text, never color alone)
- **Normal / Active** (`{colors.state-normal}` — dark `#34C972` / light `#1F9D57`)
- **Paused** (`{colors.state-paused}` — dark `#F5A623` / light `#B4790A`)
- **Error / Faulted** (`{colors.state-error}` — dark `#EF5350` / light `#D93A37`)
- **Disabled / Unknown** (`{colors.state-disabled}` — dark `#6B7686` / light `#626C7C`)
- Each state accent also has a **tint background** at 8% (light) / 12% (dark)
  alpha, e.g. `{colors.state-normal-tint}` = `#34C9721F` (dark) / `#1F9D571A`
  (light) — used as status-pill fills so full saturation stays on icon+text.

### Background & Surfaces (dark theme — default)
- `{colors.bg-top}` `#0B1220`, `{colors.bg-bottom}` `#05070C` — vertical gradient window canvas.
- `{colors.surface-card}` `#121A2C` — VRChat/Discord cards, panels.
- `{colors.surface-card-hover}` `#17203A` — hovered/pressed card & nav-row state.
- `{colors.surface-elevated}` `#1B2542` — toasts, dropdowns, dialogs.
- `{colors.border-hairline}` `#FFFFFF14` (8% white) — flat 1px hairlines.

### Background & Surfaces (light theme)
- `{colors.bg-top}` `#F3F6FC`, `{colors.bg-bottom}` `#E4E9F5`.
- `{colors.surface-card}` `#FFFFFF`.
- `{colors.surface-card-hover}` `#F3F5FA`.
- `{colors.surface-elevated}` `#FFFFFF` (stronger shadow compensates, see Elevation).
- `{colors.border-hairline}` `#00000014` (8% black).

### Text
- `{colors.text-primary}` — dark `#F2F5FA` / light `#10141F`.
- `{colors.text-secondary}` — dark `#9FB0C8` / light `#515B6E`.
- `{colors.text-disabled}` — dark `#6B7686` / light `#9AA3B2`.
- `{colors.text-on-accent-light}` `#06110E` — text/icon on bright fills (teal, green, amber solid fills).
- `{colors.text-on-accent-dark}` `#FFFFFF` — text/icon on darker fills (Discord blurple, red solid fills).

### Neutral Chrome (sidebar / focus — deliberately not service- or state-colored)
- `{colors.focus-ring}` — dark `#FFFFFF` / light `#10141F`, 2px outline at ~70% opacity. Keyboard focus only.
- `{colors.nav-active-indicator}` — dark `#F2F5FA` @ 70% / light `#10141F` @ 70% — 3px left bar on selected sidebar row.

### Border Gradients (§7.5 "thin gradient border")
- VRChat card border: `{colors.vrchat}` at 45% alpha fading to transparent.
- Discord card border: `{colors.discord}` at 45% alpha fading to transparent.
- Link-mode "Inverse/Bidirectional" segment: linear blend `{colors.vrchat}` → `{colors.discord}`, both at 45% alpha.

## Typography

**Font family:** Inter (bundled, OFL-licensed static weights — Regular/Medium/
SemiBold/Bold `.ttf`, no variable font). Renders cleanly on Windows ClearType
at small sizes; use tabular figures (`tnum`) on timestamps/port numbers so they
don't jitter. Register via Slint's font-embedding API at startup (verify exact
API against installed Slint 1.17 as an early spike).

| Token | Size | Weight | Use |
|---|---|---|---|
| `{typography.window-title}` | 14px | SemiBold | Window title |
| `{typography.nav-label}` | 13px | Medium | Sidebar nav row, default |
| `{typography.nav-label-active}` | 13px | SemiBold | Sidebar nav row, selected |
| `{typography.card-title}` | 15px | SemiBold | "VRChat"/"Discord" card headers |
| `{typography.status-primary}` | 18px | SemiBold | Glanceable state line ("Mic ON") |
| `{typography.status-secondary}` | 12px | Regular | Sub-status ("Connected") |
| `{typography.body}` | 13px | Regular | General body / settings labels |
| `{typography.caption}` | 11px | Regular, tabular figures | Timestamps, port numbers |
| `{typography.button-label}` | 13px | SemiBold | Buttons |
| `{typography.badge-label}` | 11px | SemiBold | Status pills |
| `{typography.link-glyph}` | 20px | Regular | Central ⇄ / → / ╳ connector glyph |

All sizes are logical px — Slint handles OS display-scaling automatically.
**Font-size scaling** (§7.8): a Settings text-size multiplier (Small/Default/
Large/Extra-Large → ×0.9/1.0/1.15/1.3) recomputes every `{typography.*}` token
at runtime, driven by `config.accessibility.textScale`.

## Spacing

Base unit: **4px**.

| Token | Value |
|---|---|
| `{spacing.xxs}` | 2px |
| `{spacing.xs}` | 4px |
| `{spacing.sm}` | 8px |
| `{spacing.md}` | 12px |
| `{spacing.lg}` | 16px |
| `{spacing.xl}` | 24px |
| `{spacing.xxl}` | 32px (cap — window outer margin / sidebar-to-content gap only) |

Card interior padding: `{spacing.lg}`. Gap between cards and central connector:
`{spacing.xl}`. Sidebar row padding: `{spacing.sm}` `{spacing.md}`. Button
padding: `{spacing.xs}` `{spacing.lg}`.

## Border Radius

| Token | Value | Use |
|---|---|---|
| `{rounded.xs}` | 6px | Chips, small icon buttons |
| `{rounded.sm}` | 8px | Buttons, nav-row highlight |
| `{rounded.lg}` | 14px | Cards, toasts, dialogs |
| `{rounded.pill}` | 999px | Status pills, segmented control |

## Elevation (§7.5 "控えめな影" — subtle only)

Implemented via Slint `Rectangle` drop-shadow properties, not CSS syntax.

| Token | Offset | Blur | Color (dark) | Color (light) | Use |
|---|---|---|---|---|---|
| `{elevation.flat}` | — | — | none | none | Sidebar rows, flat backgrounds |
| `{elevation.card}` | 0, 1px | 6px | `#00000040` | `#00000026` (offset 0,2px) | Resting cards |
| `{elevation.raised}` | 0, 4px | 16px | `#00000059` | `#00000033` | Toasts, dialogs, dropdowns |

## Motion

| Token | Value | Notes |
|---|---|---|
| `{motion.fade}` | 200ms ease-out | §7.5 state-change fade |
| `{motion.pulse}` | 260ms ease-out (in) / 260ms ease-in (out) | Central link pulse, only on link-state/mode change |
| `{motion.connecting-ring}` | 900ms linear, looping | Only rendered while Connecting/Authorizing |
| `{motion.reduced}` | 0ms | All of the above resolve to this when reduce-motion is enabled |

## Components

**`sidebar-nav-row`** — default: transparent bg, `{colors.text-secondary}`,
`{typography.nav-label}`. Active: `{colors.surface-card-hover}` bg, 3px left
bar `{colors.nav-active-indicator}`, `{typography.nav-label-active}`,
`{colors.text-primary}`.

**`vrchat-card`** / **`discord-card`** — `{colors.surface-card}` fill,
`{rounded.lg}`, `{elevation.card}`, 1px border gradient (own accent →
transparent), header icon+label tinted with the service accent; state text
always uses the state-accent (never the service accent).

**`primary-button`** (mode toggle segment) — selected "Inverse/Bidirectional":
teal→blurple gradient tint bg; selected "VRChat Priority": teal tint bg only;
unselected: `{colors.surface-card-hover}`; `{rounded.pill}`.

**`ghost-button`** (pause/resync) — default: 1px `{colors.border-hairline}`
outline; when representing active Paused state: `{colors.state-paused-tint}`
bg + `{colors.state-paused}` border/icon, label flips "Pause"→"Resume".

**`status-badge`** — `{rounded.pill}`, padding `{spacing.xxs}` `{spacing.sm}`,
bg = state tint, icon+text = full state accent, `{typography.badge-label}`.
One variant per state: normal/paused/error/disabled — icon and text always present.

**`link-connector`** — line color = teal→blurple gradient (bidirectional),
teal→gray dashed with right arrow (VRChat-priority), gray dashed with red ╳
(paused). Pulse only on a state transition, governed by `{motion.reduced}`.

**`toast`** — `{colors.surface-elevated}`, `{rounded.lg}`, `{elevation.raised}`,
left 3px severity bar (info=gray, warning=`{colors.state-paused}`,
error=`{colors.state-error}`), icon + text, dismiss control.

## Do's and Don'ts

### Do
- Keep the navy→black gradient still — no animated mesh, no idle motion.
- Reserve `{colors.vrchat}`/`{colors.discord}` strictly for their own service's chrome.
- Always pair a state accent with an icon AND a text label.
- Keep radii in the 6–14px range.
- Bind every `{motion.*}` duration to the reduced-motion flag (collapse to 0ms, don't remove the block).

### Don't
- Don't introduce a third hued "brand" accent for chrome.
- Don't run any animation when the window is hidden/minimized to tray.
- Don't reuse a jumbo (40px+) radius scale, web breakpoints, or oversized display typography — none of that applies to this compact native app.
