# Changelog

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
