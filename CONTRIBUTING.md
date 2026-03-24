# Contributing to Moonbeam

Thank you for your interest in contributing to Moonbeam! We welcome all contributions, including bug reports, feature requests, documentation improvements, and code changes.

## Development Workflow

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
- [Cargo](https://doc.rust-lang.org/cargo/)

### Building and Testing

To build the project:
```bash
cargo build --all-features
```

To run the test suite:
```bash
./check_all.sh
```

### Coding Style

Moonbeam follows the standard Rust formatting conventions. Please run `cargo fmt` before submitting your changes:
```bash
cargo fmt
```

We also recommend running `cargo clippy` to check for common pitfalls and idiomatic improvements:
```bash
cargo clippy --all-features
```

## Pull Request Guidelines

1. **Create a branch**: Create a new branch for your changes (`git checkout -b my-feature`).
2. **Make your changes**: Implement your feature or fix, including tests if applicable.
3. **Run tests**: Ensure all tests pass (`./check_all.sh`).
4. **Format your code**: Run `cargo fmt`.
5. **Submit a PR**: Open a Pull Request against the `main` branch with a clear description of your changes.

## Getting Help

If you have questions or need assistance, feel free to open an issue or start a discussion on GitHub.
