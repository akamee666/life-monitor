# Life Monitor Work Summary

This file is a handoff note for documentation work. It is based on the recent conversation-driven refactor and cleanup pass across the dashboard, CLI, startup flow, agent docs, and the current project structure as a whole.

It is intentionally more concrete than a release note. The goal is that another model can pick this up, understand what changed, understand why those changes were made, and then write accurate documentation without having to reconstruct the whole sequence from git history or chat logs.

## Current State At A Glance

- The project now has explicit runtime modes:
  - `life-monitor collector`
  - `life-monitor dashboard`
- The old report-style CLI analytics flow is gone.
- The ratatui dashboard is now the primary analytics/inspection interface.
- The dashboard is read-only and does not acquire the collector lock.
- Linux startup enablement now uses an interactive ratatui choice UI.
- Startup artifacts now explicitly launch `collector`.
- AGENTS.md was rewritten to match the current structure and product boundaries.
- A substantial amount of dashboard UI work happened before the CLI split, and that state is already in the codebase now.
- The project should now be documented as three separate product surfaces:
  - collector
  - dashboard / inspection
  - history movement / sync / import-export

## Project-Wide Mental Model

This project is best understood as a local-first activity recorder with a few clear layers:

1. collection
   - platform-specific input/focus collection
   - bucket buffering and segmentation
   - local SQLite writes

2. inspection
   - read-only dashboard
   - SQLite-backed analytics queries

3. history movement / convergence
   - snapshot export/import
   - optional feature-gated remote sync

The recent documentation-oriented refactor work tried to make those boundaries more explicit in:

- the CLI
- AGENTS.md
- startup behavior
- the way the dashboard is described

## Conversation-Level Summary

The work in this conversation was not one isolated patch. It was a long iterative dashboard refactor followed by a CLI and startup redesign.

The broad sequence was:

1. The dashboard was expanded and heavily refined.
2. The app activity panel, charting, summary cards, and weekly grid were repeatedly redesigned for better density, navigation, and readability.
3. The dashboard gradually replaced older report-oriented CLI analytics usage.
4. The CLI was then reshaped into explicit `collector` and `dashboard` subcommands.
5. Startup flows were updated to follow the new command structure.
6. AGENTS.md was rewritten to describe the codebase as it exists now.
7. This handoff summary was added so another model can document all of the above.

## General Codebase Structure

The current architecture is:

- `src/main.rs`
  - top-level command routing and runtime path selection
- `src/utils/args.rs`
  - clap CLI definitions and styling
- `src/common/*`
  - shared bucket, input, focus, motion, path, and ticker logic
- `src/platform/linux/*`
  - Linux input, process/focus, and startup integration
- `src/platform/windows/*`
  - Windows input, process/focus, startup, and tray integration
- `src/storage/localdb/*`
  - SQLite schema, rows, analytics, export/import, integrity
- `src/sync/*`
  - optional feature-gated multi-device sync runtime and data flow
- `src/tui/*`
  - read-only dashboard lifecycle, state, data loading, rendering
- `src/utils/lock.rs`
  - single-instance lock and DB-operation lock helpers

The most important product boundary remains:

- collection always writes to local SQLite first
- inspection reads from local SQLite
- sync/import/export operate on bucketed history rather than bypassing it

## High-Level Product Changes

- The project now has two explicit primary entrypoints:
  - `life-monitor collector`
  - `life-monitor dashboard`
- The old analytics/report CLI flow was removed in favor of the ratatui dashboard.
- The dashboard is now a first-class read-only analytics surface with richer layouts, navigation, time windows, and visualizations.
- Linux startup setup was simplified to a single enable/disable flow and now launches the `collector` subcommand explicitly.

## Important User-Visible Changes

These are the changes most likely to matter in docs, release notes, or support answers:

- Users now start collection with `life-monitor collector`.
- Users now open analytics with `life-monitor dashboard`.
- The old `--report` and `--report-days` analytics entrypoints were removed.
- The old `--tui` and `--tui-ascii` flags were removed.
- The dashboard itself now handles Unicode/ASCII switching internally.
- The dashboard includes richer status and interaction hints, so some old external CLI switches are no longer needed.

## CLI Refactor

### New command structure

The old flat CLI was replaced with clap subcommands:

- `collector`
- `dashboard`
- `sync` (feature-gated under `multi-sync`)

The root command now requires an explicit subcommand.

### Removed flags / commands

These were removed:

- `--report`
- `--report-days`
- `--tui`
- `--tui-ascii`
- Linux `--startup-mode`

Rationale:

- dashboard analytics now live behind `dashboard`
- ASCII/Unicode is now toggled inside the dashboard itself
- startup mode is chosen interactively when enabling startup on Linux

### Collector-only flags

Collector-only options now live under `life-monitor collector`, including:

- `--db-path`
- `--dpi`
- `--clear`
- import/export flags
- startup enable/disable flags
- collection interval/debug flags
- collector-side sync runtime flags when `multi-sync` is enabled

This split matters for docs because `dashboard` is intentionally inspection-only and should not imply collection or collector maintenance behavior.

### Dashboard command

`life-monitor dashboard`:

- opens the read-only ratatui dashboard
- does not start collection
- does not use the single-instance collector lock
- uses the local SQLite DB as its source of truth

### Colored help

Clap help styling was added so `--help` output is easier to scan.

### Why this refactor happened

Before this refactor, the CLI mixed collector behavior, inspection/report behavior, and dashboard toggles too loosely. The new structure makes the product model clearer:

- `collector` means write/track/manage collection-related state
- `dashboard` means inspect/analyze without collecting
- `sync` remains a separate feature-gated operational surface

That separation is now important enough to be reflected in all documentation.

## Runtime Flow Changes

The runtime model should now be described in terms of explicit execution paths.

### 1. Collector mode

`life-monitor collector`

This is the long-running writer mode. It is responsible for:

- acquiring the single-instance lock
- resolving/storing DB path configuration
- resolving/storing mouse DPI
- initializing the local database
- starting platform input collection
- starting focus/process collection
- running Windows tray behavior when applicable
- optionally running sync when compiled and configured

### 2. Dashboard mode

`life-monitor dashboard`

This is a read-only analytics mode. It:

- opens the ratatui interface
- reads dashboard aggregates from local SQLite
- refreshes periodically
- does not start collection
- does not acquire the collector lock

### 3. Short-circuit maintenance / operational commands

These still exist, but now live under the appropriate surface:

- import/export under `collector`
- startup enable/disable under `collector`
- `sync` as its own feature-gated subcommand

This matters in docs because the previous shape made it easier to mentally blur all of these together.

## Collector / Storage / Bucket Model

The underlying data model still revolves around bucket rows.

Important facts to preserve in docs:

- the collector is local-first
- SQLite is the canonical local collector database
- bucket rows remain the source of truth for analytics and dashboard views
- app usage, chart series, week activity, and summaries are all derived from bucketed data
- newer dashboard work did not replace the bucket model; it built richer views on top of it

The recent work did not fundamentally change the storage model, but documentation should be careful not to imply that the new dashboard introduced a separate analytics data layer.

## Import / Export / Sync Positioning

These areas were not deeply reworked in this last phase, but the docs/guide framing around them was updated through AGENTS.md and the CLI split.

Important current positioning:

- import/export are still collector-side maintenance operations
- they still operate against SQLite snapshots and merge safety rules
- sync remains feature-gated with `multi-sync`
- sync is still optional and should still be described as layered on top of the local-first collector

For documentation, this means:

- do not describe remote sync as the primary storage path
- do not blur dashboard analytics with sync behavior
- keep import/export/sync described as separate from dashboard inspection

## Linux Startup Flow

### New flags

Startup control is now only:

- `life-monitor collector --enable-startup`
- `life-monitor collector --disable-startup`

### Interactive Linux startup picker

When enabling startup on Linux, the user is now shown a ratatui selection UI instead of a plain terminal prompt.

The picker offers:

- `XDG autostart (recommended)`
- `systemd user service`

### Guidance text

The picker now explains the decision better:

- XDG autostart is framed as the normal choice for mainstream desktop environments such as GNOME, KDE Plasma, Xfce, Cinnamon, LXQt, MATE, and Budgie.
- systemd user service is framed as the advanced/manual choice for minimal or hand-configured sessions such as i3, sway, Hyprland, bspwm, river, awesome, and dwm.

The current implementation uses ratatui + crossterm and supports arrow keys, `j`/`k`, `Enter`, and `Esc`.

### Startup artifact behavior

Both Linux and Windows startup artifacts now launch:

- `life-monitor collector`

instead of launching the bare binary without a subcommand.

This applies to:

- Linux `.desktop` autostart entries
- Linux `systemd --user` units
- Windows Startup-folder shortcuts

### Reasoning behind the startup guidance

The picker text was deliberately rewritten to be more user-facing:

- `XDG autostart` is described as the normal/default choice for mainstream desktop environments.
- `systemd user service` is described as the advanced/manual choice for minimalist or hand-built sessions.

This was done because earlier wording was too generic and did not help users decide.

## Platform Integration Notes

Recent code changes touched both Linux and Windows startup integration.

### Linux

- startup enablement now routes through the interactive picker
- `.desktop` and `systemd --user` output now launch `collector`
- picker implementation is in `src/platform/linux/common.rs`

### Windows

- Startup-folder shortcut generation now also launches `collector`
- this was updated in `src/platform/windows/startup.rs`

So docs should describe startup in terms of “start the collector automatically”, not “start the binary”.

## Dashboard / TUI Evolution

### Dashboard positioning in the product

The dashboard is now the primary analytics UI.

The old built-in report flags were removed because the dashboard supersedes them for interactive inspection.

### Dashboard architecture

The current TUI structure is:

- `src/tui/mod.rs`: terminal lifecycle, event loop, refresh
- `src/tui/app.rs`: state machine, focus, key handling, time windows, selection/scroll
- `src/tui/data.rs`: SQLite-backed dashboard data loading and aggregation
- `src/tui/ui.rs`: ratatui layout/rendering

### Major dashboard features added or refined

- explicit time-window switching, including a new `[All]` window
- summary cards that now follow the selected window instead of always showing global all-time values
- app activity panel with:
  - full app names
  - selection/scroll support
  - scrollbar
  - per-app activity histograms
- activity chart with:
  - single-metric mode
  - multi-line scope mode
  - better titles and legends
- week activity grid with:
  - row selection
  - current-day highlighting
  - dynamic column sizing
- collector/sync status in header/footer
- inline footer hints for focused panels, including the ASCII/Unicode toggle hint

### Dashboard changes that happened before the CLI split but are now part of the current product

This conversation built on a large amount of already-in-progress dashboard work. The final state now includes all of these refinements:

- normalized metric ordering across summary cards and week activity
- compacted summary cards with centered values
- removal of redundant titles and noisy footer/header information
- a dense, space-filling layout with less dead interior panel space
- chart labels/titles rewritten to be more user-facing
- an `[All]` time window that affects:
  - app activity
  - chart data
  - summary cards
- better collector-state display in the header/footer
- quick focused-panel hints in the footer
- week activity current-day highlighting and selection behavior
- app list selection, scrolling, and scrollbar behavior
- longer, smoother per-app histogram strips rather than tiny stub sparklines
- improved multi-line chart mode with scope-like overlay behavior

If documentation explains “what the dashboard does now”, it should describe the current result, not the earlier intermediate versions.

### Removed from the dashboard

- the old categories panel was added during development and then removed
- the old focused-window footer details were removed
- stale duplicate footer/header status details were removed or consolidated

### App activity panel changes

The apps panel was heavily refactored:

- category aggregation was removed from that panel
- it now shows real app/program names again
- duration formatting was repeatedly fixed so units like `m`, `h`, `s`, and `d` stay visible
- the center visual changed from a simple proportional bar into a wider histogram/sparkline-like strip
- histogram generation was improved with:
  - more source samples
  - resampling
  - smoothing
  - better normalization
  - non-linear scaling

The visual intent is now closer to a compact analytics dashboard than a plain ranked list.

### Current apps-panel behavior

The apps panel is no longer category-oriented. It now shows app/process names directly, with:

- app label on the left
- a wide histogram strip in the middle
- share percent and duration on the right
- selection and scrolling
- scrollbar when needed

The duration formatting was a repeated source of regressions during the conversation, so docs and future changes should treat this as intentional:

- units like `m`, `h`, `s`, and `d` must remain visible
- if width is tight, the center histogram should give up room before the duration text loses units
- scrollbar gutters should not visually collide with the duration column

### Current week-activity behavior

The old `avg daily activity` naming was changed to `week activity`.

The week table now has:

- centered cell values
- centered column headers
- full metric names when width allows
- current-day row highlight when the panel is not focused
- selected-row highlight when the panel is focused
- row scrolling/selection support

The table also went through multiple layout fixes aimed at preventing large unused vertical space under the weekday rows.

### Layout refactor themes

The dashboard layout was repeatedly refactored to behave more like a dense monitoring TUI:

- less dead space inside bordered panels
- compact content-based height for the weekly table
- extra height redistributed toward the chart and app list
- scrollbars given dedicated gutters instead of overlapping text
- summary cards simplified and compacted

This is important for docs because the current layout philosophy is intentionally “space-filling” rather than fixed-height and sparse.

### Current layout intent

The intended layout policy is:

- compact tables should only get the height they can use meaningfully
- charts and scrollable lists are the primary sinks for extra height
- large empty areas inside bordered panels are considered a layout bug

This was a recurring theme in the conversation and should be reflected in any architecture or UI notes.

## AGENTS.md Rewrite

AGENTS.md was rewritten using CLAUDE.md as the baseline reference, but adjusted to match the actual codebase as it exists now.

The rewrite intentionally changed the style as well as the content:

- faster orientation up front
- practical file map near the top
- explicit runtime-path separation
- explicit TUI section
- clearer invariants
- less repetitive or overly rigid language

This matters because future models are expected to use AGENTS.md as the primary quick-orientation file, and the previous version no longer matched the refactored product shape.

## Time Window / Metrics Changes

- Added a new `[All]` dashboard time window.
- The activity chart, app list, and summary cards now all respond to the selected window.
- The single-metric chart mode was narrowed to the core activity metrics:
  - key presses
  - left clicks
  - right clicks
  - middle clicks
  - mouse movement
  - overall activity score

### Chart wording changes

The chart presentation was also made more user-facing:

- titles describe the selected mode and time range in natural language
- wording moved away from storage-oriented/internal language like “bucket”
- the graph title now carries more of the meaning directly, rather than relying on extra legend boxes

## Footer / Status Changes

- Added focused-panel quick hints in the footer.
- Added explicit ASCII/Unicode hint in the footer so users do not have to open help to discover it.
- Collector warning is now shown only when relevant.
- Collector state is surfaced directly in the header (for example `collecting`, `idle`, `stale`).

### Current status model

The header/footer now aims to answer:

- what range am I looking at?
- is collection healthy?
- is sync/local state okay?
- what can I do in the currently focused panel?

That “high-signal and compact” status philosophy is part of the current dashboard design.

## Files Most Directly Affected

If another model needs to inspect the implementation behind these docs, the highest-signal files are:

- [src/utils/args.rs](/home/ak4m3/programming/life-monitor/src/utils/args.rs)
- [src/main.rs](/home/ak4m3/programming/life-monitor/src/main.rs)
- [src/tui/mod.rs](/home/ak4m3/programming/life-monitor/src/tui/mod.rs)
- [src/tui/app.rs](/home/ak4m3/programming/life-monitor/src/tui/app.rs)
- [src/tui/data.rs](/home/ak4m3/programming/life-monitor/src/tui/data.rs)
- [src/tui/ui.rs](/home/ak4m3/programming/life-monitor/src/tui/ui.rs)
- [src/platform/linux/common.rs](/home/ak4m3/programming/life-monitor/src/platform/linux/common.rs)
- [src/platform/windows/startup.rs](/home/ak4m3/programming/life-monitor/src/platform/windows/startup.rs)
- [AGENTS.md](/home/ak4m3/programming/life-monitor/AGENTS.md)

## Commits In This Phase

Useful recent commits to inspect:

- `7776501` `Refine ratatui dashboard layout and activity views`
- `4ecf7b2` `Document intentional lock helper dead code`
- `806745e` `Refactor CLI modes and startup setup`

The last one is the main commit for the collector/dashboard split and startup picker work.

## What Still Matters Architecturally

Even after the CLI and dashboard refactors, the core product rules remain:

- collection is local-first
- bucket rows are the source of truth
- dashboard is inspection-only
- sync is feature-gated and layered on top
- platform-specific event handling stays in platform modules
- storage and merge safety remain high-risk/high-value areas

If Claude is writing project docs rather than only user docs, these are the rules worth preserving in architecture notes.

## Documentation Implications

Docs should now reflect:

- `dashboard` is the analytics mode
- `collector` is the long-running tracking mode
- import/export still live under `collector`
- Linux startup setup now includes an interactive startup-mode chooser
- startup artifacts launch `collector`
- the dashboard is read-only
- the dashboard has its own internal display toggles like ASCII/Unicode and no longer needs CLI flags for those

## Things That Were Removed Or Reframed

When updating docs, remove or rewrite references to:

- `--report`
- `--report-days`
- `--tui`
- `--tui-ascii`
- Linux `--startup-mode`
- old wording that suggests analytics reports are the main inspection UX
- old wording that suggests the dashboard is just an optional side mode

Also avoid describing the dashboard as if it still contains the categories panel. That was temporary and was removed.

## Good Documentation Targets

If writing or updating docs, the most useful places to refresh are:

- README usage examples
- CLI help examples
- startup/autostart setup section
- dashboard usage / keybindings section
- architecture notes about collector vs dashboard separation
- release notes for the CLI breaking changes
- contributor/dev docs that explain:
  - the current module map
  - the current runtime paths
  - how the dashboard relates to the collector and storage layers

## Suggested Documentation Angles

If Claude is writing docs next, the highest-value outputs would probably be:

- a README usage section that shows:
  - `life-monitor collector`
  - `life-monitor dashboard`
  - startup enable/disable examples
- a dashboard section that explains:
  - what each panel shows
  - how time windows work
  - how selection/scrolling works
  - how ASCII/Unicode toggling works
- an autostart/setup section that explains:
  - why XDG autostart is the default choice
  - when `systemd user service` is appropriate
- a short migration/release note section for old users that explains what changed from the previous CLI
- a contributor-facing architecture summary that explains:
  - collector vs dashboard vs sync surfaces
  - why bucket rows still matter
  - where startup integration lives
  - which files to read first for UI work vs collector work vs sync work
