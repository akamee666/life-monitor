# Changelog

All notable changes to this project should be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.6] - 2026-04-19

### Added

- Tag-driven release workflow that validates tags, reruns Linux and Windows checks, publishes to crates.io, creates a GitHub release, and attaches Linux and Windows binaries.
- Windows CI coverage in the non-Nix workflow.
- Snapshot export/import workflow with dry-run planning and duplicate-import protection.
- Persistent remembered database paths for custom SQLite locations and mounted shares.
- Persistent remembered DPI/CPI configuration with interactive fallback.
- `cliff.toml` configuration for generating changelog entries with `git-cliff`.

### Changed

- Storage is now local-first and SQLite-only.
- Input and focus collection now use bucket-based records instead of the old cumulative layout.
- `--db-path` now accepts either a database file or a directory, using `data.db` inside the directory when needed.
- CLI help text and user-facing storage guidance were reorganized and expanded.
- Linux and Windows input code now share more measurement logic while keeping platform-specific handling separate.
- README and AGENTS documentation now reflect the current architecture and release flow.

### Removed

- Remote backend support and its optional feature/dependencies.

## [0.1.5] - 2025-08-24

Initial crates.io release.
