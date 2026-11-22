# Folio

A standalone, offline Markdown editor for macOS, Windows, and Linux. The same Rust code handles the window contents, keyboard commands, visual formatting, and local files on all three platforms. Folio uses egui and eframe with the operating system’s window and file-dialog backends.

![Folio in Write mode](screenshots/write.png)

## Run from source

Install stable Rust and the C linker for your platform (Xcode Command Line Tools on macOS, Visual Studio C++ Build Tools on Windows). On Ubuntu, install the build prerequisites:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libgl1-mesa-dev libx11-dev libxcb1-dev
```

From this checkout, run `cargo run --release --locked`. To open an existing document at startup, append `-- path/to/notes.md`. The first build downloads dependencies. The installed editor works offline.

## Editing

Open local files through **File → Open** or drag them into the window. Each document has its own tab. Choose **Write** for visual formatting or **Source** for literal Markdown. The outline jumps to headings, and Find selects the next occurrence.

| Command | Mac | Windows / Linux |
|---|---|---|
| Open / Save | ⌘O / ⌘S | Ctrl+O / Ctrl+S |
| Write / Source | ⌘/ | Ctrl+/ |
| Heading | ⌘1–3 | Ctrl+1–3 |
| Bold / Italic | ⌘B / ⌘I | Ctrl+B / Ctrl+I |
| Toggle task | ⇧⌘T | Ctrl+Shift+T |

Named documents autosave after two idle seconds. New documents ask for a destination when saved. If a file changes on disk, Folio stops the save and lets you save a separate copy. Unsupported Markdown syntax stays in the file.

## How it works

[The editor](src/main.rs) feeds one editable string to egui. [The layout code](src/window.rs) styles that string while preserving character positions for the caret. [The document code](src/document.rs) translates selections to UTF-8 byte offsets, edits Markdown, and uses a temporary file in the destination directory for atomic replacement. [The Markdown rules](src/lib.rs) handle block prefixes, inline formatting, code fences, BOMs and line endings.

Run `cargo test --locked` for the editing and file tests. Run `cargo run --release --locked -- --self-test` to exercise both editor layouts and a disk round trip. On Apple Silicon, `./package.sh` builds a drag-to-Applications disk image. Linux and Windows packages include the executable and licenses.

[Contributing](CONTRIBUTING.md) · [MIT license](LICENSE) · [Ubuntu font license](assets/fonts/UFL.txt)
