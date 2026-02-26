# Contributing to Apex

Thank you for your interest in contributing to Apex! We welcome contributions from everyone.

## Getting Started

1.  **Fork the repository** on GitHub.
2.  **Clone your fork** locally:
    ```bash
    git clone https://github.com/your-username/apex.git
    cd apex
    ```
3.  **Create a branch** for your feature or bug fix:
    ```bash
    git checkout -b feature/my-new-feature
    ```

## Development Workflow

### Prerequisites

-   Rust (latest stable)
-   Docker (optional, for running e2e tests)

### Running Tests

Run unit and integration tests:

```bash
cargo test
```

Run end-to-end tests (requires python):

```bash
python3 tests/e2e/run_e2e.py
```

### Code Style

We follow standard Rust coding conventions. Please ensure your code is formatted and linted before submitting:

```bash
cargo fmt --all -- --check
cargo clippy -- -D warnings
```

## Submitting a Pull Request

1.  Ensure all tests pass.
2.  Update documentation if necessary.
3.  Push your branch to GitHub.
4.  Open a Pull Request against the `main` branch.
5.  Provide a clear description of your changes.

## Reporting Issues

Please use the [Issue Tracker](https://github.com/your-org/apex/issues) to report bugs or request features.
Use the provided templates to ensure all necessary information is included.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
