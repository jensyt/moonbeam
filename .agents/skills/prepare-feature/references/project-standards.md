# Project Standards for Moonbeam

This document outlines the standards for project-level documentation.

## CHANGELOG.md

We follow the [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) standard.

### Section Order
- `Added`: For new features.
- `Changed`: For changes in existing functionality.
- `Deprecated`: For soon-to-be-removed features.
- `Removed`: For now-removed features.
- `Fixed`: For any bug fixes.
- `Security`: In case of vulnerabilities.

### Guidelines
- **Unreleased**: All new changes must be listed under the `## [Unreleased]` section.
- **Breaking Changes**: Clearly label breaking changes with **BREAKING**.
- **User-Centric**: Write descriptions that are meaningful to the users of the library, not just the developers.

## AGENTS.md

`AGENTS.md` is the "source of truth" for AI agents working on this project. It should be kept up to date with:
- **Project Overview**: High-level summary.
- **Architecture**: Core design principles (e.g., "Single-threaded by default").
- **Workspace Structure**: Description of crates and their purposes.
- **Key Components**: Major features or DSLs (e.g., `router!` macro).

## README.md

The README should focus on:
- **Getting Started**: Quick start examples.
- **Features**: List of main capabilities.
- **Motivation/Philosophy**: Why this project exists.
- **Performance**: Benchmarks if relevant.

## todo.md

- **Todo**: List of outstanding tasks or planned features.
- **Done**: List of completed tasks, moved from the Todo section.

## Rust Documentation (rustdoc)

- All **public** items (modules, structs, enums, functions, traits) must have a doc comment (`///`).
- Include code examples in doc comments for complex APIs.
- Use `doc_cfg` attributes for feature-gated APIs where possible.
