# Changelog

## v1.4.0

**Removed**
- "Save as..." in the detail view — redundant with copying the file
  directly, removed per feedback rather than kept as unused clutter

**Fixed**
- Tiles no longer crop a gif vertically when its aspect ratio is extreme
  enough to hit the tile-height clamp — switched from Cover to Contain, so
  the whole frame stays visible (letterboxed instead of cropped) rather
  than losing the top/bottom of a tall gif
- The grid now adapts its column count to the actual window width instead
  of always being a fixed 3 columns

**Library**
- Clicking a popular tag now swaps the search to just that tag instead of
  stacking onto whatever was already there — it's a quick "jump to this
  category" shortcut, not another AND criterion, so browsing between
  favorites can't accidentally empty the results. Clicking the active one
  clears it; the active tag is highlighted.
- Media filter: All / Gifs / Images, next to the sort buttons
- "Add gif" now stands out with a "+" and the theme's accent color
- Removed the per-tile cloud/disk source-of-import icon — not something
  worth the visual noise

**Stats**
- Breakdown of animated (.gif) vs static image (png/jpg/webp) counts

## v1.3.0

- "Reset filters" button — clears search criteria and puts sort back to
  Newest in one click, instead of removing each search pill by hand
- Popular-tag chips moved off the sort row and onto the same row as "Add
  gif" (right-aligned), so they're not confused with the Newest/Oldest/etc.
  sort controls
- "Save as..." in the detail view: saves a copy of the gif/image to
  wherever you pick, working for any of the supported formats

## v1.2.0

**Memory**
- Fixed a real memory leak: decoded gif frames were never freed after a
  tile scrolled off screen, so RAM only ever grew. Scrolling out now frees
  them again — scrolling back is a fast disk-cache read, not a re-decode

**Fixes**
- The "Edit tags" button in the detail view could get pushed off-screen;
  the detail overlay now has a fixed width like the other modals

**Search**
- The library search box is now tag pills, not free text: type a tag,
  press space (or Tab to accept a suggested match) to add it as a
  criterion, add more the same way — results need *all* of them, not just
  one. Typing without pressing space still live-filters as before
- Popular-tag chips add to the search criteria instead of replacing them

**Manage tags**
- Collapsed by default
- Search box to find a tag in a long list
- Sort by name / most used / least used
- Delete a tag outright (removes it from every gif that has it, with a
  confirmation first)
- The list itself is height-capped with its own scrollbar once expanded

**Images**
- Funny pictures aren't always animated — PNG, JPEG and WebP are now
  supported everywhere a gif was (file picker, drag & drop, URL import,
  with the URL path correctly detecting the real format instead of always
  assuming .gif)

## v1.1.0

**Look and feel**
- Custom Discord-inspired dark/light theme (blurple accent, layered
  backgrounds instead of one flat gray) instead of iced's stock Light/Dark
- Fira Sans as the default font everywhere
- Tiles now show a real hover highlight and rounded corners

**Tags**
- Edit an existing gif's tags from the detail view (was import-only before)
- Rename or merge tags from Settings → Manage tags (renaming to an existing
  tag's name merges the two, without creating duplicate tag/gif pairs)

**Safety**
- "Delete forever" in the trash now asks for confirmation first
- Esc closes whichever modal/overlay is currently open

**Bulk actions**
- "Select" mode in the library: multi-select tiles, tag all of them at
  once, or move them all to trash in one action

**Import**
- Drag a `.gif` file onto the window to open the import dialog with it
  pre-selected, instead of only through the file picker

## v1.0.2

- Fix: on Windows, launching `GifVault.exe` opened an extra console window
  alongside the app, and closing that console killed the app with it. The
  binary now builds with the `windows` subsystem instead of the default
  `console` one, so only the actual GUI window appears.

## v1.0.1

- Fix: the Windows build failed to compile (`clipboard-win`'s `set_clipboard`
  convenience function can't be used with `FileList`, which only implements
  `Setter` for an unsized slice). Copying a gif file to the clipboard on
  Windows now opens the clipboard and calls the `Setter` trait method
  directly instead.

## v1.0.0

First tagged release.

- Tiled, ratio-aware library grid with animated playback for visible tiles
- Tags with prefix autocomplete and popular-tag quick filters
- Trash (soft delete, restore, permanent delete, empty)
- Stats screen, light/dark theme, toast notifications
- Click-to-copy: copies the actual gif file to the clipboard, not a link
- Backup: export/import the whole library as a `.zip`
- Lazy, cached, parallel gif decoding — the library no longer decodes
  anything that isn't actually on screen, and never re-decodes the whole
  collection on a single add/trash/restore
- GitHub Actions release pipeline building macOS (`.dmg`) and Windows
  (`.exe`) on every version tag
