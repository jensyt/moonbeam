---
name: prepare-feature
description: Prepares a new feature or bug fix for commit in the Moonbeam project. Use this when you have finished implementing a feature and need to ensure all documentation, tests, and examples are up to date and passing.
---

# Prepare Feature

Use this skill to systematically wrap up a feature or bug fix for the Moonbeam project. It ensures that the repository remains in a clean, documented, and well-tested state.

## Workflow

Follow these steps in order to prepare your changes for a commit.

### 1. Analyze Changes
Identify all modified files and determine their impact on the project's public API, core architecture, or existing functionality.

### 2. Update Documentation
Systematically update the following files:
- **`CHANGELOG.md`**: Add concise entries under the `[Unreleased]` section following the [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) format. Categorize as `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, or `Security`.
- **`README.md`**: Update if the changes affect user-facing features, installation, or configuration. Make sure all crate READMEs are updated.
- **`AGENTS.md`**: Update if the architecture, core philosophy, or workspace structure has changed.
- **`todo.md`**: Move completed tasks to `Done` and add any newly discovered debt to `Todo`.
- **Rust Docs**: Ensure all new public modules, structs, and functions have triple-slash (`///`) doc comments. Use `cargo clippy -p moonbeam --all-features -- -D missing-docs` to verify.

### 3. Ensure Test Coverage
- Verify that new logic is covered by unit tests in the same file or integration tests in `tests/integration`.
- If a new feature was added, create a corresponding integration test in `tests/integration/tests/`.
- Refer to [references/testing-standards.md](references/testing-standards.md) for details.

### 4. Validate with `check_all.sh`
- If new features or crates were added, update `check_all.sh` to include them.
- Run `./check_all.sh` and ensure it passes. This runs clippy and tests across all feature combinations.

### 5. Add Examples
- Ask the user if the feature requires a new example in `examples/`.
- If yes, implement a minimal, idiomatic example demonstrating the new functionality.

## Resources

- [references/project-standards.md](references/project-standards.md): Standards for CHANGELOG, README, and other project-level docs.
- [references/testing-standards.md](references/testing-standards.md): Guidelines for unit and integration tests.
