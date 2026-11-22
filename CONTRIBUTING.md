# Contributing to Folio

Fork the repository, create a branch from main, and open a pull request with one focused change. For a new dependency or a substantial feature, open an issue first so we can discuss its effect on file fidelity, app size, and the editing experience.

For a bug, include the smallest Markdown example that reproduces it, your macOS version, and what you expected. Use synthetic examples when a document contains private information.

## Before opening a pull request

Run these checks on an Apple Silicon Mac with the build prerequisites in the README and the rustfmt component installed:

```sh
cargo fmt --check
cargo test --locked
./build.sh
target/Folio.app/Contents/MacOS/Folio --self-test
```

Add a focused regression check for changes to editing or serialization. For UI changes, exercise the affected workflow in the app and include a screenshot when it helps reviewers. Describe what changed, why, and which checks you ran.

Changes to main go through pull requests. The macOS check must pass and review conversations must be resolved before merging.
