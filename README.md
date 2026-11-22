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

## Package for macOS

Run `./package.sh` on Apple Silicon to build a versioned disk image in `target/`. Open the image and drag Folio into Applications. The package includes the MIT license. Local builds use an ad hoc signature. Set `SIGN_IDENTITY` to a Developer ID Application identity to sign the packaged app. Notarization is a separate release step.

## How it works

Folio keeps one editable string containing the original Markdown. The native AppKit text view applies fonts, colors, and paragraph spacing directly to ranges of that string. Write mode gives recognized headings and emphasis their visual styling. Source mode displays the same characters in monospace. Switching views never converts the document through HTML.

The Rust function in [src/lib.rs](src/lib.rs) recognizes heading, quote, and bullet prefixes. Swift passes each line as UTF-8 bytes through [Folio.h](Folio.h). Because those prefixes are ASCII, their byte lengths also work as offsets in the native text view. The [document controller](main.swift) handles selections, formatting commands, the heading outline, and file I/O.

Try typing `Field notes`, pressing **⌘1**, then **⌘/**. The source shows `# Field notes`. Save with **⌘S** and open the file in another text editor to see those same characters.
