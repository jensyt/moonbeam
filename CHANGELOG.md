# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **BREAKING**: `moonbeam::assets::get_asset` is now an `async` function.
- `get_asset` now offloads all blocking filesystem operations to a background thread pool, preventing the main executor from stalling and improving throughput for single-threaded mode by ~66%.

### Added
- "Small file" optimization in `get_asset`: Files under 16KB are now read entirely into memory immediately to avoid streaming overhead.
- Basic performance benchmarks in the `README.md` comparing Moonbeam to Node.js and Rouille.

## [0.3.6] - 2026-03-24

### Added
- **Documentation**: Enhanced `docs.rs` integration. Added `doc_cfg` attributes to all feature-gated APIs, allowing users to see required features directly in the documentation.
- **Internal**: Bumped `moonbeam-attributes` to `v0.1.3` with matching `docs.rs` improvements.

## [0.3.5] - 2026-03-23

### Added
- Initial open source documentation structure.
- Comprehensive `README.md` with motivation and caveats.
- `LICENSE` (MIT).
- `CONTRIBUTING.md`.
- `SECURITY.md` for vulnerability reporting.
- Doc comments for all public APIs in `moonbeam` and `moonbeam-attributes`.
- Examples for stateless, stateful, and multi-threaded server configurations.
