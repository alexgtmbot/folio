# Folio

Open a Markdown file, edit it visually, and save it in place. Folio is an Apple Silicon Mac app built with Swift/AppKit and a small Rust component for Markdown block markers.

![Folio editing a Markdown document in Write mode](screenshots/write.png)

## Build and run

On an Apple Silicon Mac, install Xcode Command Line Tools and a stable Rust toolchain with Cargo (Rust 2024 edition). From the project directory, run:

```sh
./build.sh
open target/Folio.app
```

Use **File → Open** to open a UTF-8 Markdown document and **⌘S** to save. Use **Format** to apply headings, emphasis, lists, and quotes. **⌘/** switches between Write and [Source view](screenshots/source.png). The outline jumps to headings. Tables and images remain editable as source.

See [contributing](CONTRIBUTING.md) for development checks. Folio is [MIT licensed](LICENSE).
