# Testing Standards for Moonbeam

Moonbeam emphasizes high reliability through unit and integration testing.

## Unit Tests

Unit tests are written in the same file as the code they test, using a `cfg(test)` module.

### Guidelines
- **Internal Logic**: Use unit tests to verify internal logic of modules.
- **Mocking**: Use `piper::pipe` to simulate network streams where applicable.
- **Async**: Use `async_io::block_on` or equivalent for async tests.

## Integration Tests

Integration tests live in `tests/integration/tests/`.

### Guidelines
- **Public API**: Use integration tests to verify the public API and core server behavior.
- **Scenario-Based**: Each test should cover a complete scenario (e.g., a full request/response cycle with a router).
- **Features**: Ensure integration tests work with different feature combinations.

## check_all.sh

`check_all.sh` is the primary validation tool. It ensures the project builds and tests pass for various feature combinations.

### Updating check_all.sh
- **New Features**: If you add a new feature, add it to the `FEATURES` array in `check_all.sh`.
- **New Crates**: If you add a new crate to the workspace, add a `cargo check -p <crate-name>` or similar entry.
- **New Logical Groups**: If a new feature interacts with existing ones in complex ways, add a new "Logical Feature Group" check.

## Clippy & Formatting

- Always run `cargo fmt` before committing.
- Clippy should be run with `--all-features` and should not have any warnings (`-D warnings`).
- For public APIs, use `cargo clippy -p moonbeam --all-features -- -D missing-docs` to ensure documentation.
