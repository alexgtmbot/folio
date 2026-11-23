<p align="center"><img src="assets/Folio.svg" width="104" height="104" alt="Folio’s cream notebook with a vermilion bookmark"></p>

<h1 align="center">Folio</h1>
<p align="center">A tiny, offline Markdown editor for macOS, Windows, and Linux.</p>
<p align="center"><a href="https://github.com/alexgtmbot/folio/releases/latest"><img alt="Download the latest Folio release" src="https://img.shields.io/github/v/release/alexgtmbot/folio?label=Download&amp;color=306c66"></a> <a href="https://github.com/alexgtmbot/folio/actions/workflows/check.yml"><img alt="Build and test status" src="https://github.com/alexgtmbot/folio/actions/workflows/check.yml/badge.svg?branch=main"></a> <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-35625b"></a></p>

Open a file, format it as you write, and save ordinary Markdown. Folio runs in its own window and keeps your documents on your computer.

**[Download Folio](https://github.com/alexgtmbot/folio/releases/latest)** · Apple Silicon disk image (signed and notarized), Windows x64 ZIP, or Linux x64 archive with build provenance.

## Around the editor

| Write | Source |
| --- | --- |
| [![Write view with the outline and formatted field notes](screenshots/write.png)](screenshots/write.png) | [![Source view showing the same field notes as Markdown](screenshots/source.png)](screenshots/source.png) |
| Choose a heading in the left outline to jump to that section. Use **Format** to style the current line or selection. | Switch modes at the bottom right to see the characters Folio saves, including heading markers, emphasis, and tasks. |

## Run from source

Install stable Rust and the C linker for your platform (Xcode Command Line Tools on macOS, Visual Studio C++ Build Tools on Windows). On Ubuntu, install the build prerequisites:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libgl1-mesa-dev libx11-dev libxcb1-dev
```

From this checkout, run `cargo run --release --locked`. To open an existing document at startup, append `-- path/to/notes.md`. The first build downloads dependencies. The installed editor works offline. On Linux, provide a Wayland or X11 desktop, OpenGL, and a desktop file-picker portal. The X11 backend requires libxkbcommon-x11 (on Ubuntu, `libxkbcommon-x11-0`).

## Editing

Open local files through **File → Open** or drag them into the window. Each document has its own tab. Choose **Write** to edit alongside a live CommonMark preview, or **Source** for the single-pane Markdown editor. The preview renders nested lists, tables, task lists, fenced code, blockquotes, links, local images, strikethrough, footnotes, and definition lists. Raw HTML is displayed as text. The outline jumps to headings, and Find selects the next occurrence.

In Write mode, type **/** at the beginning of a line and filter by name (for example, **/table**). Use ↑ or ↓ to select, Enter or Tab to insert, or Escape to keep the literal text. Slash commands are disabled inside code blocks.

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

## From GTM.bot

[![AskWinnie](https://img.shields.io/badge/AskWinnie-Parenting-74495e)](https://askwinnie.app) [![WithMarsha](https://img.shields.io/badge/WithMarsha-DBT_skills-365943)](https://withmarsha.app) [![BetaFlow](https://img.shields.io/badge/BetaFlow-Climbing-415970)](https://betaflow.app) [![GTM.bot](https://img.shields.io/badge/GTM.bot-Studio-953d24)](https://gtm.bot)
