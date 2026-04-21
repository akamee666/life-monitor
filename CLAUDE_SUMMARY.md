# Life Monitor Work Summary

This file is a handoff note for documentation work. It summarizes the major product and codebase changes completed in the recent TUI and CLI refactors so another model can write accurate docs, release notes, or onboarding material without re-discovering everything from git history.

## High-Level Product Changes

- The project now has two explicit primary entrypoints:
  - `life-monitor collector`
  - `life-monitor dashboard`
- The old analytics/report CLI flow was removed in favor of the ratatui dashboard.
- The dashboard is now a first-class read-only analytics surface with richer layouts, navigation, time windows, and visualizations.
- Linux startup setup was simplified to a single enable/disable flow and now launches the `collector` subcommand explicitly.

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

### Layout refactor themes

The dashboard layout was repeatedly refactored to behave more like a dense monitoring TUI:

- less dead space inside bordered panels
- compact content-based height for the weekly table
- extra height redistributed toward the chart and app list
- scrollbars given dedicated gutters instead of overlapping text
- summary cards simplified and compacted

This is important for docs because the current layout philosophy is intentionally “space-filling” rather than fixed-height and sparse.

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

## Footer / Status Changes

- Added focused-panel quick hints in the footer.
- Added explicit ASCII/Unicode hint in the footer so users do not have to open help to discover it.
- Collector warning is now shown only when relevant.
- Collector state is surfaced directly in the header (for example `collecting`, `idle`, `stale`).

## Documentation Implications

Docs should now reflect:

- `dashboard` is the analytics mode
- `collector` is the long-running tracking mode
- import/export still live under `collector`
- Linux startup setup now includes an interactive startup-mode chooser
- startup artifacts launch `collector`
- the dashboard is read-only
- the dashboard has its own internal display toggles like ASCII/Unicode and no longer needs CLI flags for those

## Good Documentation Targets

If writing or updating docs, the most useful places to refresh are:

- README usage examples
- CLI help examples
- startup/autostart setup section
- dashboard usage / keybindings section
- architecture notes about collector vs dashboard separation
- release notes for the CLI breaking changes
