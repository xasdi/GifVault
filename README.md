# GifVault

[![Release](https://github.com/xasdi/GifVault/actions/workflows/release.yml/badge.svg)](https://github.com/xasdi/GifVault/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/xasdi/GifVault)](../../releases/latest)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-blue)
![Rust](https://img.shields.io/badge/rust-2024_edition-orange)
![Built with iced](https://img.shields.io/badge/gui-iced-6c5ce7)

A fast, local gif library manager. Import your gifs, tag them, find them again,
and paste them — the actual animated file, not a link — anywhere in one click.

## What it does

- **Tiled library** — a Pinterest-style grid sized to each gif's aspect ratio,
  playing the actual animation (not just a static thumbnail) for whatever's
  on screen
- **Tags with autocomplete** — start typing a tag, get suggested the closest
  match from what you've already used, Tab to accept it; quick-filter chips
  for your most-used tags sit right under the search box
- **Click to copy** — clicking a gif copies the *file itself* to your
  clipboard, so it pastes straight into Discord, Slack, iMessage, wherever —
  not a dead link to a path only your computer can see
- **Trash, not deletion** — removing a gif moves it to a trash you can
  restore from, until you empty it for good
- **Stats** — library size, where your gifs came from, how many times
  you've copied one out, how much space the trash is holding onto
- **Backup** — export the whole library to a single `.zip`, import it back
  on another machine
- **Light/dark theme**, because it's 2026 and half of you refuse to use
  anything with a white background

## Installation

No Rust, no terminal, no build step. Grab a file, run it.

1. Go to the [**Releases**](../../releases) page.
2. Under the newest release, download:
   - **macOS** → `GifVault-macOS.dmg`
   - **Windows** → `GifVault-Windows.exe`

### macOS

Open the `.dmg`, drag **GifVault** into your **Applications** folder.

The first time you open it, macOS will refuse, saying it can't verify the
developer — that's expected, this isn't signed with a paid Apple developer
certificate. **Right-click the app → Open → Open** (instead of double-clicking)
the first time, and it'll never ask again.

### Windows

Just run the `.exe` — nothing to install, nothing to unzip.

Windows SmartScreen will likely throw up a blue "Windows protected your PC"
screen the first time, for the same reason as above (no paid code-signing
certificate). Click **More info → Run anyway**.

## For the curious (technical details)

GifVault is a native desktop app written in **Rust**, using
[**iced**](https://iced.rs) for the GUI (rendered via `wgpu`, falling back to
a software rasterizer when there's no GPU to talk to) and a bundled
**SQLite** database for everything that isn't a gif file on disk.

A few things worth knowing if you're the type who checks:

- **Decoding is lazy.** A gif's pixel dimensions are peeked from its file
  header at import time — cheap, no frame decoding involved — so the grid
  can lay every tile out correctly before a single frame has ever been
  decoded. Frames only get decoded once a tile actually scrolls into view;
  everything off-screen is handed to the renderer as a plain file path and
  costs nothing until it's looked at.
- **Decoded frames are cached to disk**, downscaled and pre-processed, next
  to the source file. The first time you scroll past a gif, it decodes.
  Every time after that — including across restarts — it just reads the
  cache back.
- **Newly-visible batches decode in parallel**, one thread per gif, instead
  of one at a time.
- **Nothing reloads the whole library.** Adding, trashing, or restoring a
  gif mutates the in-memory state directly — the app doesn't re-read (and
  re-decode) the entire collection just because one item changed.
- **Animation only ticks when something's actually animating on screen** —
  not forever in the background, not while you're sitting in Settings.
- Clicking a gif on **macOS** copies the file via the same clipboard
  mechanism as Cmd+C on a file in Finder (`osascript`); on **Windows** it
  writes a `CF_HDROP` file list directly via `clipboard-win`. Same result —
  paste it anywhere a file drop works.
- Releases are built automatically by GitHub Actions the moment a version
  tag is pushed: `.dmg` for macOS via `cargo-bundle`, a portable `.exe` for
  Windows, both attached straight to the GitHub Release.

Built with a fair amount of back-and-forth on making a Rust GUI app that
handles "a few hundred animated gifs" without turning your laptop into a
space heater. If you're curious how deep that rabbit hole goes, the commit
history has the whole story.

### By the numbers

Measured, not guessed, during development:

| | |
|---|---|
| Release binary (macOS) | ~22 MB, ~18 MB stripped |
| CPU while the grid is idle (nothing animating on screen) | ~0% |
| CPU while actively animating the visible tiles | ~4–5% sustained |
| CPU before the animation-tick bug was fixed | ~33% sustained, *all the time*, even on the Settings screen |
| Decoding the heaviest gif in dev testing (91 frames, 498×445) | 838 ms release build, 16.3 s unoptimized debug build |
| Same gif, second time (disk cache hit) | no decode at all — straight to a memory read |
| Gifs decoded at startup, regardless of library size | 0 — only what's actually scrolled into view |
