// Without this, Windows launches a console window alongside the GUI one
// (the default "console" subsystem), and closing it kills the app. A no-op
// on every other platform, so it's safe to leave unconditional.
#![windows_subsystem = "windows"]

mod db;
mod model;
mod thumbnail;

use iced::widget::image::Handle;
use iced::widget::{
    button, center, column, container, image, mouse_area, opaque, row, scrollable, stack, text,
    text_input, tooltip,
};
use iced::keyboard;
use iced::theme::Palette;
use iced::window::Settings as WindowSettings;
use iced::{
    color, Bottom, Center, Color, ContentFit, Element, Font, Length, Size, Subscription, Task,
    Theme,
};
use model::Gif;
use rusqlite::Connection;
use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thumbnail::GifAnimation;

const TAG_DRAFT_INPUT_ID: &str = "tag-draft-input";

const GRID_COLUMNS: usize = 3;
const TILE_WIDTH: f32 = 280.0;
const TILE_SPACING: f32 = 15.0;
const MIN_TILE_HEIGHT: f32 = 140.0;
const MAX_TILE_HEIGHT: f32 = 420.0;
// Tiles within this many pixels of the viewport are also kept animated, so
// playback doesn't visibly pause right as a tile scrolls into view.
const VISIBILITY_MARGIN: f32 = 400.0;
const POPULAR_TAGS_LIMIT: usize = 8;
const TOAST_DURATION_MS: u64 = 2200;

#[derive(Debug, Clone, PartialEq)]
enum ImportMode {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ViewMode {
    Library,
    Trash,
    Stats,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortMode {
    DateNewest,
    DateOldest,
    MostUsed,
    LeastUsed,
}

impl SortMode {
    const ALL: [SortMode; 4] = [
        SortMode::DateNewest,
        SortMode::DateOldest,
        SortMode::MostUsed,
        SortMode::LeastUsed,
    ];

    fn label(self) -> &'static str {
        match self {
            SortMode::DateNewest => "Newest",
            SortMode::DateOldest => "Oldest",
            SortMode::MostUsed => "Most used",
            SortMode::LeastUsed => "Least used",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AppTheme {
    Light,
    Dark,
}

impl AppTheme {
    fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("light") => AppTheme::Light,
            _ => AppTheme::Dark,
        }
    }

    fn as_setting(self) -> &'static str {
        match self {
            AppTheme::Light => "light",
            AppTheme::Dark => "dark",
        }
    }

    /// A custom palette instead of iced's built-in Light/Dark, tuned closer
    /// to a modern chat-app look (dark backgrounds a few shades apart from
    /// each other rather than one flat gray, a blurple accent).
    fn palette(self) -> Palette {
        match self {
            AppTheme::Dark => Palette {
                background: color!(0x31_33_38),
                text: color!(0xdb_de_e1),
                primary: color!(0x58_65_f2),
                success: color!(0x23_a5_5a),
                warning: color!(0xf0_b2_32),
                danger: color!(0xed_42_45),
            },
            AppTheme::Light => Palette {
                background: color!(0xff_ff_ff),
                text: color!(0x06_06_07),
                primary: color!(0x58_65_f2),
                success: color!(0x23_a5_5a),
                warning: color!(0xa9_7c_17),
                danger: color!(0xda_37_3c),
            },
        }
    }

    fn to_iced_theme(self) -> Theme {
        let name = match self {
            AppTheme::Dark => "GifVault Dark",
            AppTheme::Light => "GifVault Light",
        };

        Theme::custom(name, self.palette())
    }

    /// A background a couple of shades apart from the main content, used
    /// for the sidebar and other "recessed" panels — the layered look most
    /// modern chat/tool apps use instead of one flat background everywhere.
    fn recessed_background(self) -> Color {
        match self {
            AppTheme::Dark => color!(0x2b_2d_31),
            AppTheme::Light => color!(0xf2_f3_f5),
        }
    }
}

/// Represents a gif that has been successfully staged (either picked from
/// disk and copied/moved, or downloaded from a URL) and is ready to be
/// reviewed in the modal before being saved to the database.
struct StagedGif {
    source_type: String,
    source_path: String,
    stored_path: PathBuf,
}

struct GifEntry {
    gif: Gif,
    animation: Option<GifAnimation>,
    tags: Vec<String>,
    current_frame: usize,
    frame_elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct LibraryStats {
    gif_count: usize,
    total_size_bytes: u64,
    local_count: usize,
    url_count: usize,
    total_copies: i64,
    trash_count: usize,
    trash_size_bytes: u64,
}

struct App {
    conn: Connection,
    entries: Vec<GifEntry>,
    deleted_entries: Vec<GifEntry>,
    view_mode: ViewMode,
    sort_mode: SortMode,
    app_theme: AppTheme,
    filter_input: String,
    show_import_modal: bool,
    pending_file: Option<PathBuf>,
    staged_gif: Option<StagedGif>,
    pending_thumbnail: Option<Handle>,
    staged_tags: Vec<String>,
    tag_draft: String,
    tag_suggestion: Option<String>,
    tag_stats: Vec<(String, i64)>,
    import_mode: ImportMode,
    url_input: String,
    is_fetching_url: bool,
    import_error: Option<String>,
    detail_gif_id: Option<i64>,
    scroll_offset: f32,
    viewport_height: f32,
    visible_gif_ids: HashSet<i64>,
    last_tick: Option<Instant>,
    toast: Option<(String, Instant)>,
    pending_import_zip: Option<PathBuf>,
    stats: LibraryStats,
    pending_permanent_delete: Option<i64>,
    /// Which gif's tags are currently being edited in the detail view, if
    /// any — reuses `staged_tags`/`tag_draft`/`tag_suggestion` (the same
    /// pill editor as the import flow), since only one of the two can ever
    /// be open at once.
    editing_tags_for: Option<i64>,
    renaming_tag: Option<String>,
    tag_rename_draft: String,
    selection_mode: bool,
    selected_ids: HashSet<i64>,
}

#[derive(Debug, Clone)]
enum Message {
    FilterChanged(String),
    ShowImportModal,
    HideImportModal,
    PickFile,
    FileSelected(Option<PathBuf>),
    UrlInputChanged(String),
    FetchUrl,
    UrlFetched(Result<PathBuf, String>),
    TagDraftChanged(String),
    CommitTagDraft,
    RemoveStagedTag(usize),
    TagInputKey(keyboard::Event),
    ModalKeyPressed(keyboard::Event),
    PopularTagClicked(String),
    ImportModeChanged(ImportMode),
    ConfirmImport,
    MoveToTrash(i64),
    RestoreGif(i64),
    RequestPermanentDelete(i64),
    CancelPermanentDelete,
    PermanentlyDeleteGif(i64),
    EmptyTrash,
    CopyGifFile(i64),
    ShowDetail(i64),
    HideDetail,
    EditTagsClicked(i64),
    SaveTagsEdit(i64),
    CancelTagsEdit,
    StartRenameTag(String),
    TagRenameDraftChanged(String),
    ConfirmRenameTag,
    CancelRenameTag,
    FileDropped(PathBuf),
    ToggleSelectionMode,
    ToggleTileSelected(i64),
    BulkTagDraftChanged(String),
    ApplyBulkTag,
    BulkMoveToTrash,
    Scrolled(scrollable::Viewport),
    Tick(Instant),
    SetViewMode(ViewMode),
    SortModeChanged(SortMode),
    ThemeChanged(AppTheme),
    ExportBackup,
    ExportDestinationChosen(Option<PathBuf>),
    ImportBackupClicked,
    ImportZipChosen(Option<PathBuf>),
    ConfirmImportBackup,
    CancelImportBackup,
}

/// Where a tile lands in the masonry grid: which column, how far down it
/// sits within that column, and how tall it is.
struct TilePlacement {
    column: usize,
    y_offset: f32,
    height: f32,
}

/// Greedily places each tile (in order) into the shortest column so far,
/// producing a Pinterest-style masonry layout.
fn compute_layout(heights: &[f32]) -> Vec<TilePlacement> {
    let mut column_heights = [0.0f32; GRID_COLUMNS];

    heights
        .iter()
        .map(|&height| {
            let column = column_heights
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(index, _)| index)
                .unwrap();

            let y_offset = column_heights[column];
            column_heights[column] += height + TILE_SPACING;

            TilePlacement {
                column,
                y_offset,
                height,
            }
        })
        .collect()
}

impl App {
    fn new() -> Self {
        let conn = db::init_db();
        let entries = Self::load_entries(&conn);
        let deleted_entries =
            Self::build_entries(&conn, db::queries::list_deleted_gifs(&conn).unwrap_or_default());
        let app_theme =
            AppTheme::from_setting(db::queries::get_setting(&conn, "theme").ok().flatten().as_deref());

        let mut app = Self {
            conn,
            entries,
            deleted_entries,
            view_mode: ViewMode::Library,
            sort_mode: SortMode::DateNewest,
            app_theme,
            filter_input: String::new(),
            show_import_modal: false,
            pending_file: None,
            staged_gif: None,
            pending_thumbnail: None,
            staged_tags: Vec::new(),
            tag_draft: String::new(),
            tag_suggestion: None,
            tag_stats: Vec::new(),
            import_mode: ImportMode::Copy,
            url_input: String::new(),
            is_fetching_url: false,
            import_error: None,
            detail_gif_id: None,
            scroll_offset: 0.0,
            // No scroll event has fired yet, so assume a generous viewport
            // to avoid punishing tiles that are visible on first render.
            viewport_height: 900.0,
            visible_gif_ids: HashSet::new(),
            last_tick: None,
            toast: None,
            pending_import_zip: None,
            stats: LibraryStats::default(),
            pending_permanent_delete: None,
            editing_tags_for: None,
            renaming_tag: None,
            tag_rename_draft: String::new(),
            selection_mode: false,
            selected_ids: HashSet::new(),
        };
        Self::backfill_dimensions_for(&app.conn, &mut app.entries);
        Self::backfill_dimensions_for(&app.conn, &mut app.deleted_entries);
        app.recompute_visible();
        app.refresh_tag_stats();
        app
    }

    /// Builds entries without decoding any gif frames — dimensions (needed
    /// for grid layout) already live in the database, so a tile can be laid
    /// out and shown as a static preview (via `Handle::from_path`) long
    /// before it's ever actually decoded. Real per-frame decoding only
    /// happens once a tile is scrolled into view — see `ensure_decoded`.
    fn build_entries(conn: &Connection, gifs: Vec<Gif>) -> Vec<GifEntry> {
        gifs.into_iter()
            .map(|gif| {
                let tags = db::queries::list_tags_for_gif(conn, gif.id).unwrap_or_default();
                GifEntry {
                    gif,
                    animation: None,
                    tags,
                    current_frame: 0,
                    frame_elapsed_ms: 0,
                }
            })
            .collect()
    }

    /// One-time fixup for gifs imported before dimensions were tracked in
    /// the database (width/height default to 0 on migration). Peeking a
    /// header is cheap, so this is fine to do for the whole library at
    /// startup — it does not decode any frames.
    fn backfill_dimensions_for(conn: &Connection, entries: &mut [GifEntry]) {
        for entry in entries.iter_mut() {
            if entry.gif.width > 0 && entry.gif.height > 0 {
                continue;
            }

            let path =
                entry.gif.local_cache_path.clone().unwrap_or_else(|| entry.gif.source_path.clone());

            if let Some((width, height)) = thumbnail::peek_dimensions(&path) {
                let _ = db::queries::set_gif_dimensions(conn, entry.gif.id, width, height);
                entry.gif.width = width as i64;
                entry.gif.height = height as i64;
            }
        }
    }

    /// Decodes (in parallel, across as many gifs as needed) whichever of the
    /// given gif ids don't already have their frames decoded. Called right
    /// after a batch of tiles becomes visible, so the library never pays for
    /// decoding gifs that are never actually scrolled to.
    fn ensure_decoded(&mut self, ids: &[i64]) {
        let cache_dir = db::cache_dir();

        let to_decode: Vec<(usize, i64, String)> = ids
            .iter()
            .filter_map(|&id| {
                let index = self.entries.iter().position(|entry| entry.gif.id == id)?;
                if self.entries[index].animation.is_some() {
                    return None;
                }
                let path = self.entries[index]
                    .gif
                    .local_cache_path
                    .clone()
                    .unwrap_or_else(|| self.entries[index].gif.source_path.clone());
                Some((index, id, path))
            })
            .collect();

        if to_decode.is_empty() {
            return;
        }

        let decoded: Vec<(usize, Option<GifAnimation>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = to_decode
                .iter()
                .map(|(index, id, path)| {
                    let cache_dir = &cache_dir;
                    scope.spawn(move || {
                        (*index, thumbnail::load_animation_cached(*id, path, cache_dir))
                    })
                })
                .collect();

            handles.into_iter().map(|handle| handle.join().unwrap()).collect()
        });

        for (index, animation) in decoded {
            if animation.is_some() {
                self.entries[index].animation = animation;
            }
        }
    }

    fn load_entries(conn: &Connection) -> Vec<GifEntry> {
        Self::build_entries(conn, db::queries::list_gifs(conn).unwrap_or_default())
    }

    fn load_deleted_entries(&mut self) {
        let gifs = db::queries::list_deleted_gifs(&self.conn).unwrap_or_default();
        self.deleted_entries = Self::build_entries(&self.conn, gifs);
    }

    /// Decodes and inserts just the newly-inserted gif, instead of
    /// reloading (and re-decoding every frame of) the whole library.
    fn push_new_entry(&mut self, gif_id: i64) {
        let Ok(gif) = db::queries::get_gif(&self.conn, gif_id) else {
            return;
        };

        if let Some(entry) = Self::build_entries(&self.conn, vec![gif]).into_iter().next() {
            self.entries.insert(0, entry);
        }
    }

    fn reset_modal_state(&mut self) {
        self.show_import_modal = false;
        self.pending_file = None;
        self.staged_gif = None;
        self.pending_thumbnail = None;
        self.staged_tags.clear();
        self.tag_draft.clear();
        self.tag_suggestion = None;
        self.import_mode = ImportMode::Copy;
        self.url_input.clear();
        self.is_fetching_url = false;
        self.import_error = None;
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let filter = self.filter_input.trim().to_lowercase();

        let mut indices: Vec<usize> = if filter.is_empty() {
            (0..self.entries.len()).collect()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&filter))
                })
                .map(|(index, _)| index)
                .collect()
        };

        indices.sort_by(|&a, &b| {
            let a = &self.entries[a].gif;
            let b = &self.entries[b].gif;

            match self.sort_mode {
                SortMode::DateNewest => b.added_at.cmp(&a.added_at),
                SortMode::DateOldest => a.added_at.cmp(&b.added_at),
                SortMode::MostUsed => b.use_count.cmp(&a.use_count),
                SortMode::LeastUsed => a.use_count.cmp(&b.use_count),
            }
        });

        indices
    }

    /// Dimensions come from the database (peeked at import time), not from
    /// decoding — so layout never has to wait on, or trigger, a decode.
    fn tile_height_for(entry: &GifEntry) -> f32 {
        let width = entry.gif.width.max(1) as f32;
        let height = entry.gif.height.max(1) as f32;

        (TILE_WIDTH * height / width).clamp(MIN_TILE_HEIGHT, MAX_TILE_HEIGHT)
    }

    /// Small badge shown on a tile to indicate whether a gif came from a
    /// URL or was imported from a local folder.
    fn source_icon(entry: &GifEntry) -> &'static str {
        if entry.gif.source_type == "url" {
            "☁"
        } else {
            "💾"
        }
    }

    fn refresh_tag_stats(&mut self) {
        self.tag_stats = db::queries::list_tag_stats(&self.conn).unwrap_or_default();
    }

    fn files_total_size(entries: &[GifEntry]) -> u64 {
        entries
            .iter()
            .filter_map(|entry| entry.gif.local_cache_path.as_deref())
            .filter_map(|path| std::fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .sum()
    }

    /// Recomputes the numbers shown on the Stats screen from what's already
    /// loaded in memory (entries + deleted_entries), plus a disk size lookup
    /// per file. Cheap enough to call on every navigation to that screen.
    fn refresh_stats(&mut self) {
        let local_count =
            self.entries.iter().filter(|entry| entry.gif.source_type != "url").count();

        self.stats = LibraryStats {
            gif_count: self.entries.len(),
            total_size_bytes: Self::files_total_size(&self.entries),
            local_count,
            url_count: self.entries.len() - local_count,
            total_copies: self.entries.iter().map(|entry| entry.gif.use_count).sum(),
            trash_count: self.deleted_entries.len(),
            trash_size_bytes: Self::files_total_size(&self.deleted_entries),
        };
    }

    fn format_bytes(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{bytes} B")
        } else {
            format!("{size:.1} {}", UNITS[unit_index])
        }
    }

    /// The most-used tags, for the quick-filter chips under the search box.
    fn popular_tags(&self) -> Vec<&str> {
        let mut stats: Vec<&(String, i64)> =
            self.tag_stats.iter().filter(|(_, count)| *count > 0).collect();
        stats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        stats
            .into_iter()
            .take(POPULAR_TAGS_LIMIT)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Recomputes the best autocomplete suggestion for the tag currently
    /// being typed, preferring the most-used matching tag.
    fn update_tag_suggestion(&mut self) {
        let draft = self.tag_draft.trim().to_lowercase();

        if draft.is_empty() {
            self.tag_suggestion = None;
            return;
        }

        let mut candidates: Vec<&(String, i64)> = self
            .tag_stats
            .iter()
            .filter(|(name, _)| {
                name.to_lowercase().starts_with(&draft)
                    && !self
                        .staged_tags
                        .iter()
                        .any(|staged| staged.eq_ignore_ascii_case(name))
            })
            .collect();

        candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        self.tag_suggestion = candidates.first().map(|(name, _)| name.clone());
    }

    fn push_staged_tag(&mut self, tag: &str) {
        let tag = tag.trim();

        if tag.is_empty() {
            return;
        }

        if self.staged_tags.iter().any(|staged| staged.eq_ignore_ascii_case(tag)) {
            return;
        }

        self.staged_tags.push(tag.to_string());
    }

    /// Commits whatever is currently typed in the tag draft as a new pill.
    fn commit_tag_draft(&mut self) {
        let tag = std::mem::take(&mut self.tag_draft);
        self.push_staged_tag(&tag);
        self.tag_suggestion = None;
    }

    fn show_toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    /// Recomputes which gifs currently fall within (or near) the visible
    /// viewport, so only those keep animating.
    fn recompute_visible(&mut self) {
        let indices = self.filtered_indices();
        let heights: Vec<f32> = indices
            .iter()
            .map(|&index| Self::tile_height_for(&self.entries[index]))
            .collect();
        let placements = compute_layout(&heights);

        let top = self.scroll_offset - VISIBILITY_MARGIN;
        let bottom = self.scroll_offset + self.viewport_height + VISIBILITY_MARGIN;

        let new_visible: HashSet<i64> = indices
            .iter()
            .zip(placements.iter())
            .filter(|(_, placement)| {
                placement.y_offset + placement.height >= top && placement.y_offset <= bottom
            })
            .map(|(&index, _)| self.entries[index].gif.id)
            .collect();

        let newly_visible: Vec<i64> =
            new_visible.iter().filter(|id| !self.visible_gif_ids.contains(id)).copied().collect();

        self.visible_gif_ids = new_visible;

        if !newly_visible.is_empty() {
            self.ensure_decoded(&newly_visible);
        }
    }

    fn store_local_file(original: &PathBuf, mode: &ImportMode) -> Result<PathBuf, String> {
        let extension = original
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("gif");

        let new_name = format!("{}.{}", uuid::Uuid::new_v4(), extension);
        let destination = db::gifs_storage_dir().join(new_name);

        std::fs::copy(original, &destination)
            .map_err(|e| format!("Failed to copy file: {e}"))?;

        if *mode == ImportMode::Move {
            std::fs::remove_file(original)
                .map_err(|e| format!("Copy succeeded but failed to remove original: {e}"))?;
        }

        Ok(destination)
    }

    async fn download_gif(url: String) -> Result<PathBuf, String> {
        let response = reqwest::get(&url)
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!("Server returned status {}", response.status()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))?;

        let destination = db::gifs_storage_dir().join(format!("{}.gif", uuid::Uuid::new_v4()));

        std::fs::write(&destination, &bytes)
            .map_err(|e| format!("Failed to save downloaded file: {e}"))?;

        Ok(destination)
    }

    /// Bundles the database and every stored gif file into a single zip, so
    /// the whole library can be backed up or moved to another computer.
    fn write_backup_zip(destination: &Path) -> Result<(), String> {
        let file = std::fs::File::create(destination).map_err(|e| e.to_string())?;
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let db_bytes = std::fs::read(db::db_path())
            .map_err(|e| format!("Failed to read database: {e}"))?;
        writer
            .start_file("gifs.db", options)
            .map_err(|e| e.to_string())?;
        writer.write_all(&db_bytes).map_err(|e| e.to_string())?;

        let gifs_dir = db::gifs_storage_dir();
        for entry in std::fs::read_dir(&gifs_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                continue;
            }

            let bytes = std::fs::read(entry.path()).map_err(|e| e.to_string())?;
            let name = format!("gifs/{}", entry.file_name().to_string_lossy());
            writer.start_file(name, options).map_err(|e| e.to_string())?;
            writer.write_all(&bytes).map_err(|e| e.to_string())?;
        }

        writer.finish().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Replaces the current database and gif files with the contents of a
    /// backup zip (see [`Self::write_backup_zip`]). Destructive: meant to be
    /// called only after the user has confirmed the overwrite.
    fn restore_backup(&mut self, zip_path: &Path) -> Result<(), String> {
        let data_dir = db::gifs_storage_dir()
            .parent()
            .ok_or("Could not determine app data directory")?
            .to_path_buf();
        // Extracted on the same volume as the app data dir so the gif files
        // can be moved into place with a cheap rename instead of a copy.
        let temp_dir = data_dir.join(format!(".restore-{}", uuid::Uuid::new_v4()));

        let zip_file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| e.to_string())?;
        archive.extract(&temp_dir).map_err(|e| e.to_string())?;

        let extracted_db = temp_dir.join("gifs.db");
        if !extracted_db.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err("The zip file doesn't contain a gifs.db".to_string());
        }

        let target_gifs_dir = db::gifs_storage_dir();
        let _ = std::fs::remove_dir_all(&target_gifs_dir);
        std::fs::create_dir_all(&target_gifs_dir).map_err(|e| e.to_string())?;

        let extracted_gifs_dir = temp_dir.join("gifs");
        if extracted_gifs_dir.exists() {
            for entry in std::fs::read_dir(&extracted_gifs_dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let dest = target_gifs_dir.join(entry.file_name());
                std::fs::rename(entry.path(), dest).map_err(|e| e.to_string())?;
            }
        }

        std::fs::copy(&extracted_db, db::db_path()).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(&temp_dir);

        self.conn = db::init_db();
        self.entries = Self::load_entries(&self.conn);
        self.load_deleted_entries();
        self.app_theme = AppTheme::from_setting(
            db::queries::get_setting(&self.conn, "theme").ok().flatten().as_deref(),
        );
        self.recompute_visible();
        self.refresh_tag_stats();

        Ok(())
    }

    /// Puts the actual gif file on the system clipboard (the same way
    /// Finder's Cmd+C on a file does), so it can be pasted directly into
    /// chat apps, Mail, Notes, etc. without losing the animation.
    #[cfg(target_os = "macos")]
    fn copy_file_to_clipboard(path: &str) -> bool {
        let script = format!(
            "set the clipboard to (POSIX file \"{}\")",
            path.replace('\\', "\\\\").replace('"', "\\\"")
        );

        match std::process::Command::new("osascript").arg("-e").arg(script).output() {
            Ok(output) if output.status.success() => true,
            Ok(output) => {
                eprintln!(
                    "Failed to copy file to clipboard: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                false
            }
            Err(err) => {
                eprintln!("Failed to copy file to clipboard: {err}");
                false
            }
        }
    }

    // NOTE: written from documentation/memory, not verified on a real
    // Windows machine or toolchain (none available in this environment) —
    // please sanity-check that pasting actually works before relying on it.
    #[cfg(target_os = "windows")]
    fn copy_file_to_clipboard(path: &str) -> bool {
        // `set_clipboard` (the convenience free function) can't be used
        // here: `FileList` only implements `Setter<[T]>` (an unsized
        // slice), but `set_clipboard`'s `data` parameter is taken by value
        // and requires `Sized`. Opening the clipboard and calling the
        // trait method directly (which takes `&[T]`) sidesteps that.
        use clipboard_win::{formats::FileList, Clipboard, Setter};

        // Kept alive (as `_clipboard`) for the duration of the write; the
        // clipboard closes automatically when it drops at the end of scope.
        let _clipboard = match Clipboard::new_attempts(10) {
            Ok(clipboard) => clipboard,
            Err(err) => {
                eprintln!("Failed to open clipboard: {err}");
                return false;
            }
        };

        match FileList.write_clipboard(&[path]) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("Failed to copy file to clipboard: {err}");
                false
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn copy_file_to_clipboard(_path: &str) -> bool {
        eprintln!("Copying a file to the clipboard isn't implemented on this platform yet");
        false
    }

    fn theme(&self) -> Theme {
        self.app_theme.to_iced_theme()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = Vec::new();

        // Ticking is only useful while gif tiles are actually animating on
        // screen; without this, the app kept redrawing at ~25fps forever —
        // even while sitting on the Trash/Stats/Settings screen — because
        // `entries` is non-empty the moment you've imported anything.
        let animating = self.view_mode == ViewMode::Library && !self.visible_gif_ids.is_empty();

        if animating || self.toast.is_some() {
            subs.push(iced::time::every(Duration::from_millis(50)).map(Message::Tick));
        }

        // Only listen for Tab while there is something to complete, so we
        // don't hijack Tab-based focus navigation the rest of the time.
        if (self.show_import_modal || self.editing_tags_for.is_some()) && self.tag_suggestion.is_some()
        {
            subs.push(keyboard::listen().map(Message::TagInputKey));
        }

        if self.any_modal_open() {
            subs.push(keyboard::listen().map(Message::ModalKeyPressed));
        }

        // Always on: dropping a .gif file onto the window opens the import
        // modal with it pre-selected, same as picking it from a dialog.
        subs.push(iced::window::events().filter_map(|(_id, event)| match event {
            iced::window::Event::FileDropped(path) => Some(Message::FileDropped(path)),
            _ => None,
        }));

        Subscription::batch(subs)
    }

    fn any_modal_open(&self) -> bool {
        self.detail_gif_id.is_some()
            || self.pending_import_zip.is_some()
            || self.show_import_modal
            || self.pending_permanent_delete.is_some()
    }

    /// Closes whichever overlay is currently on top, matching the priority
    /// order `view()` uses to decide which one to render.
    fn close_topmost_modal(&mut self) {
        if self.editing_tags_for.is_some() {
            self.editing_tags_for = None;
        } else if self.detail_gif_id.is_some() {
            self.detail_gif_id = None;
        } else if self.pending_import_zip.is_some() {
            self.pending_import_zip = None;
        } else if self.show_import_modal {
            self.reset_modal_state();
        } else if self.pending_permanent_delete.is_some() {
            self.pending_permanent_delete = None;
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FilterChanged(value) => {
                self.filter_input = value;
                self.recompute_visible();
            }
            Message::ShowImportModal => {
                self.reset_modal_state();
                self.show_import_modal = true;
            }
            Message::HideImportModal => {
                self.reset_modal_state();
            }
            Message::PickFile => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("GIF", &["gif"])
                            .pick_file()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::FileSelected,
                );
            }
            Message::FileSelected(path) => {
                if let Some(path) = path {
                    let path_str = path.to_string_lossy().to_string();
                    self.pending_thumbnail = thumbnail::load_animation(&path_str)
                        .map(|animation| animation.frames[0].clone());
                    self.pending_file = Some(path);
                    self.import_error = None;
                }
            }
            Message::UrlInputChanged(value) => {
                self.url_input = value;
            }
            Message::FetchUrl => {
                if self.url_input.trim().is_empty() {
                    return Task::none();
                }

                self.is_fetching_url = true;
                self.import_error = None;
                let url = self.url_input.trim().to_string();

                return Task::perform(Self::download_gif(url), Message::UrlFetched);
            }
            Message::UrlFetched(result) => {
                self.is_fetching_url = false;

                match result {
                    Ok(stored_path) => {
                        let stored_path_str = stored_path.to_string_lossy().to_string();
                        self.pending_thumbnail = thumbnail::load_animation(&stored_path_str)
                            .map(|animation| animation.frames[0].clone());
                        self.staged_gif = Some(StagedGif {
                            source_type: "url".to_string(),
                            source_path: self.url_input.trim().to_string(),
                            stored_path,
                        });
                    }
                    Err(err) => {
                        self.import_error = Some(err);
                    }
                }
            }
            Message::TagDraftChanged(value) => {
                if value.contains(',') {
                    let parts: Vec<&str> = value.split(',').collect();
                    let last = parts.len() - 1;

                    for part in &parts[..last] {
                        self.push_staged_tag(part);
                    }

                    self.tag_draft = parts[last].to_string();
                } else {
                    self.tag_draft = value;
                }

                self.update_tag_suggestion();
            }
            Message::CommitTagDraft => {
                self.commit_tag_draft();
            }
            Message::RemoveStagedTag(index) => {
                if index < self.staged_tags.len() {
                    self.staged_tags.remove(index);
                }
            }
            Message::TagInputKey(event) => {
                if let keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Tab),
                    ..
                } = event
                {
                    if let Some(suggestion) = self.tag_suggestion.clone() {
                        self.tag_draft = suggestion;
                        self.update_tag_suggestion();
                        // Otherwise the cursor stays where "c" ended and typing
                        // resumes mid-word instead of after the completed tag.
                        return iced::widget::operation::move_cursor_to_end(TAG_DRAFT_INPUT_ID);
                    }
                }
            }
            Message::ModalKeyPressed(event) => {
                if let keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                } = event
                {
                    self.close_topmost_modal();
                }
            }
            Message::PopularTagClicked(tag_name) => {
                self.filter_input = tag_name;
                self.recompute_visible();
            }
            Message::ImportModeChanged(mode) => {
                self.import_mode = mode;
            }
            Message::ConfirmImport => {
                // Case 1: a gif downloaded from a URL is already staged and
                // stored on disk, nothing left to do but save it to the DB.
                if let Some(staged) = self.staged_gif.take() {
                    let stored_path_str = staged.stored_path.to_string_lossy().to_string();

                    let gif_id = db::queries::insert_gif(
                        &self.conn,
                        &staged.source_type,
                        &staged.source_path,
                        &stored_path_str,
                    )
                    .expect("could not insert gif");

                    db::queries::set_tags_for_gif(&self.conn, gif_id, &self.staged_tags.join(","))
                        .expect("could not save tags");

                    if let Some((width, height)) = thumbnail::peek_dimensions(&stored_path_str) {
                        let _ = db::queries::set_gif_dimensions(&self.conn, gif_id, width, height);
                    }

                    self.push_new_entry(gif_id);
                    self.reset_modal_state();
                    self.recompute_visible();
                    self.refresh_tag_stats();
                    self.show_toast("Gif added");
                    return Task::none();
                }

                // Case 2: a local file was picked and still needs to be
                // copied or moved into the app's storage folder.
                if let Some(path) = self.pending_file.clone() {
                    let source_path_str = path.to_string_lossy().to_string();

                    match Self::store_local_file(&path, &self.import_mode) {
                        Ok(stored_path) => {
                            let stored_path_str = stored_path.to_string_lossy().to_string();

                            let gif_id = db::queries::insert_gif(
                                &self.conn,
                                "local",
                                &source_path_str,
                                &stored_path_str,
                            )
                            .expect("could not insert gif");

                            db::queries::set_tags_for_gif(
                                &self.conn,
                                gif_id,
                                &self.staged_tags.join(","),
                            )
                            .expect("could not save tags");

                            if let Some((width, height)) =
                                thumbnail::peek_dimensions(&stored_path_str)
                            {
                                let _ =
                                    db::queries::set_gif_dimensions(&self.conn, gif_id, width, height);
                            }

                            self.push_new_entry(gif_id);
                            self.reset_modal_state();
                            self.recompute_visible();
                            self.refresh_tag_stats();
                            self.show_toast("Gif added");
                        }
                        Err(err) => {
                            self.import_error = Some(err);
                        }
                    }
                }
            }
            Message::MoveToTrash(gif_id) => {
                // The file on disk is left alone so the gif can still be
                // restored later; only permanent deletion removes it. The
                // entry is moved between the in-memory lists directly
                // (instead of reloading and re-decoding everything) so this
                // stays instant regardless of library size.
                match db::queries::soft_delete_gif(&self.conn, gif_id) {
                    Ok(()) => {
                        if let Some(pos) = self.entries.iter().position(|entry| entry.gif.id == gif_id) {
                            let mut entry = self.entries.remove(pos);
                            if let Ok(gif) = db::queries::get_gif(&self.conn, gif_id) {
                                entry.gif = gif;
                            }
                            self.deleted_entries.insert(0, entry);
                        }
                        self.detail_gif_id = None;
                        self.recompute_visible();
                        self.refresh_tag_stats();
                        self.show_toast("Moved to trash");
                    }
                    Err(err) => eprintln!("Failed to trash gif {gif_id}: {err}"),
                }
            }
            Message::RestoreGif(gif_id) => {
                match db::queries::restore_gif(&self.conn, gif_id) {
                    Ok(()) => {
                        if let Some(pos) =
                            self.deleted_entries.iter().position(|entry| entry.gif.id == gif_id)
                        {
                            let mut entry = self.deleted_entries.remove(pos);
                            if let Ok(gif) = db::queries::get_gif(&self.conn, gif_id) {
                                entry.gif = gif;
                            }
                            self.entries.insert(0, entry);
                        }
                        self.recompute_visible();
                        self.refresh_tag_stats();
                        self.show_toast("Restored");
                    }
                    Err(err) => eprintln!("Failed to restore gif {gif_id}: {err}"),
                }
            }
            Message::RequestPermanentDelete(gif_id) => {
                self.pending_permanent_delete = Some(gif_id);
            }
            Message::CancelPermanentDelete => {
                self.pending_permanent_delete = None;
            }
            Message::PermanentlyDeleteGif(gif_id) => {
                self.pending_permanent_delete = None;

                if let Some(pos) =
                    self.deleted_entries.iter().position(|entry| entry.gif.id == gif_id)
                {
                    let entry = self.deleted_entries.remove(pos);
                    if let Some(path) = &entry.gif.local_cache_path {
                        let _ = std::fs::remove_file(path);
                    }
                    thumbnail::remove_cache(gif_id, &db::cache_dir());
                }

                match db::queries::permanently_delete_gif(&self.conn, gif_id) {
                    Ok(()) => {
                        self.refresh_tag_stats();
                        self.show_toast("Deleted permanently");
                    }
                    Err(err) => eprintln!("Failed to permanently delete gif {gif_id}: {err}"),
                }
            }
            Message::EmptyTrash => {
                let cache_dir = db::cache_dir();
                for entry in self.deleted_entries.drain(..) {
                    if let Some(path) = &entry.gif.local_cache_path {
                        let _ = std::fs::remove_file(path);
                    }
                    thumbnail::remove_cache(entry.gif.id, &cache_dir);
                    let _ = db::queries::permanently_delete_gif(&self.conn, entry.gif.id);
                }

                self.refresh_tag_stats();
                self.show_toast("Trash emptied");
            }
            Message::CopyGifFile(gif_id) => {
                let path = self
                    .entries
                    .iter()
                    .find(|entry| entry.gif.id == gif_id)
                    .and_then(|entry| entry.gif.local_cache_path.clone());

                if let Some(path) = path {
                    if Self::copy_file_to_clipboard(&path) {
                        if let Some(entry) =
                            self.entries.iter_mut().find(|entry| entry.gif.id == gif_id)
                        {
                            entry.gif.use_count += 1;
                        }
                        let _ = db::queries::increment_use_count(&self.conn, gif_id);
                        self.recompute_visible();
                        self.show_toast("Copied to clipboard");
                    }
                }
            }
            Message::ShowDetail(gif_id) => {
                self.detail_gif_id = Some(gif_id);
                self.editing_tags_for = None;
            }
            Message::HideDetail => {
                self.detail_gif_id = None;
                self.editing_tags_for = None;
            }
            Message::EditTagsClicked(gif_id) => {
                if let Some(entry) = self.entries.iter().find(|entry| entry.gif.id == gif_id) {
                    self.staged_tags = entry.tags.clone();
                    self.tag_draft.clear();
                    self.tag_suggestion = None;
                    self.editing_tags_for = Some(gif_id);
                }
            }
            Message::CancelTagsEdit => {
                self.editing_tags_for = None;
            }
            Message::SaveTagsEdit(gif_id) => {
                // Whatever's still sitting in the draft box counts too —
                // saving shouldn't silently drop a tag the user typed but
                // never pressed Enter/comma on.
                self.commit_tag_draft();

                match db::queries::set_gif_tags_exact(&self.conn, gif_id, &self.staged_tags) {
                    Ok(()) => {
                        if let Some(entry) =
                            self.entries.iter_mut().find(|entry| entry.gif.id == gif_id)
                        {
                            entry.tags = self.staged_tags.clone();
                        }
                        self.refresh_tag_stats();
                        self.editing_tags_for = None;
                        self.show_toast("Tags updated");
                    }
                    Err(err) => eprintln!("Failed to save tags for gif {gif_id}: {err}"),
                }
            }
            Message::StartRenameTag(name) => {
                self.renaming_tag = Some(name.clone());
                self.tag_rename_draft = name;
            }
            Message::CancelRenameTag => {
                self.renaming_tag = None;
                self.tag_rename_draft.clear();
            }
            Message::TagRenameDraftChanged(value) => {
                self.tag_rename_draft = value;
            }
            Message::ConfirmRenameTag => {
                if let Some(old_name) = self.renaming_tag.take() {
                    let new_name = self.tag_rename_draft.trim().to_string();

                    if !new_name.is_empty() && new_name != old_name {
                        match db::queries::rename_or_merge_tag(&self.conn, &old_name, &new_name) {
                            Ok(()) => {
                                for entry in self.entries.iter_mut().chain(&mut self.deleted_entries)
                                {
                                    for tag in entry.tags.iter_mut() {
                                        if *tag == old_name {
                                            *tag = new_name.clone();
                                        }
                                    }
                                    entry.tags.sort();
                                    entry.tags.dedup();
                                }
                                self.refresh_tag_stats();
                                self.show_toast("Tag renamed");
                            }
                            Err(err) => eprintln!("Failed to rename tag {old_name}: {err}"),
                        }
                    }
                }

                self.tag_rename_draft.clear();
            }
            Message::FileDropped(path) => {
                if path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("gif")).unwrap_or(false) {
                    self.reset_modal_state();
                    self.show_import_modal = true;
                    let path_str = path.to_string_lossy().to_string();
                    self.pending_thumbnail = thumbnail::load_animation(&path_str)
                        .map(|animation| animation.frames[0].clone());
                    self.pending_file = Some(path);
                    self.import_error = None;
                }
            }
            Message::ToggleSelectionMode => {
                self.selection_mode = !self.selection_mode;
                self.selected_ids.clear();
                self.tag_draft.clear();
                self.tag_suggestion = None;
            }
            Message::ToggleTileSelected(gif_id) => {
                if !self.selected_ids.insert(gif_id) {
                    self.selected_ids.remove(&gif_id);
                }
            }
            Message::BulkTagDraftChanged(value) => {
                self.tag_draft = value;
                self.update_tag_suggestion();
            }
            Message::ApplyBulkTag => {
                let tag = self.tag_draft.trim().to_string();
                self.tag_draft.clear();
                self.tag_suggestion = None;

                if tag.is_empty() || self.selected_ids.is_empty() {
                    return Task::none();
                }

                for &gif_id in &self.selected_ids {
                    if let Some(entry) = self.entries.iter_mut().find(|entry| entry.gif.id == gif_id)
                    {
                        if !entry.tags.iter().any(|existing| existing.eq_ignore_ascii_case(&tag)) {
                            entry.tags.push(tag.clone());
                            entry.tags.sort();
                        }
                        let _ = db::queries::set_gif_tags_exact(&self.conn, gif_id, &entry.tags);
                    }
                }

                self.refresh_tag_stats();
                self.show_toast(format!("Tagged {} gifs", self.selected_ids.len()));
            }
            Message::BulkMoveToTrash => {
                let ids: Vec<i64> = self.selected_ids.drain().collect();
                let count = ids.len();

                for gif_id in ids {
                    if let Some(pos) = self.entries.iter().position(|entry| entry.gif.id == gif_id) {
                        let mut entry = self.entries.remove(pos);
                        if let Ok(gif) = db::queries::get_gif(&self.conn, gif_id) {
                            entry.gif = gif;
                        }
                        let _ = db::queries::soft_delete_gif(&self.conn, gif_id);
                        self.deleted_entries.insert(0, entry);
                    }
                }

                self.selection_mode = false;
                self.recompute_visible();
                self.refresh_tag_stats();
                self.show_toast(format!("Moved {count} gifs to trash"));
            }
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
                if mode == ViewMode::Stats {
                    self.refresh_stats();
                }
            }
            Message::SortModeChanged(mode) => {
                self.sort_mode = mode;
                self.recompute_visible();
            }
            Message::ThemeChanged(theme) => {
                self.app_theme = theme;
                let _ = db::queries::set_setting(&self.conn, "theme", theme.as_setting());
                self.show_toast("Theme updated");
            }
            Message::ExportBackup => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_file_name("gifvault-backup.zip")
                            .add_filter("Zip archive", &["zip"])
                            .save_file()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::ExportDestinationChosen,
                );
            }
            Message::ExportDestinationChosen(path) => {
                if let Some(path) = path {
                    match Self::write_backup_zip(&path) {
                        Ok(()) => self.show_toast("Backup exported"),
                        Err(err) => {
                            eprintln!("Backup export failed: {err}");
                            self.show_toast("Backup export failed");
                        }
                    }
                }
            }
            Message::ImportBackupClicked => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Zip archive", &["zip"])
                            .pick_file()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::ImportZipChosen,
                );
            }
            Message::ImportZipChosen(path) => {
                self.pending_import_zip = path;
            }
            Message::CancelImportBackup => {
                self.pending_import_zip = None;
            }
            Message::ConfirmImportBackup => {
                if let Some(path) = self.pending_import_zip.take() {
                    match self.restore_backup(&path) {
                        Ok(()) => self.show_toast("Library restored from backup"),
                        Err(err) => {
                            eprintln!("Backup import failed: {err}");
                            self.show_toast("Backup import failed");
                        }
                    }
                }
            }
            Message::Scrolled(viewport) => {
                self.scroll_offset = viewport.absolute_offset().y;
                self.viewport_height = viewport.bounds().height;
                self.recompute_visible();
            }
            Message::Tick(now) => {
                let elapsed_ms = self
                    .last_tick
                    .map(|last| (now - last).as_millis() as u64)
                    .unwrap_or(0);
                self.last_tick = Some(now);

                let toast_expired = self.toast.as_ref().is_some_and(|(_, shown_at)| {
                    now.duration_since(*shown_at) >= Duration::from_millis(TOAST_DURATION_MS)
                });
                if toast_expired {
                    self.toast = None;
                }

                for entry in self.entries.iter_mut() {
                    if !self.visible_gif_ids.contains(&entry.gif.id) {
                        continue;
                    }

                    let Some(animation) = entry.animation.as_ref() else {
                        continue;
                    };
                    if animation.frames.len() <= 1 {
                        continue;
                    }

                    entry.frame_elapsed_ms += elapsed_ms;
                    while entry.frame_elapsed_ms >= animation.delays_ms[entry.current_frame] {
                        entry.frame_elapsed_ms -= animation.delays_ms[entry.current_frame];
                        entry.current_frame = (entry.current_frame + 1) % animation.frames.len();
                    }
                }
            }
        }

        Task::none()
    }

    fn build_tile(entry: &GifEntry, height: f32, selection_mode: bool, is_selected: bool) -> Element<Message> {
        let gif_id = entry.gif.id;

        let image_element: Element<Message> = match &entry.animation {
            Some(animation) if !animation.frames.is_empty() => {
                let frame_index = entry.current_frame.min(animation.frames.len() - 1);
                image(animation.frames[frame_index].clone())
                    .width(Length::Fixed(TILE_WIDTH))
                    .height(Length::Fixed(height))
                    .content_fit(ContentFit::Cover)
                    .border_radius(8.0)
                    .into()
            }
            // Not decoded (yet): hand the raw path to iced, which decodes
            // and caches a static preview lazily on its own, off our plate
            // entirely, instead of us decoding frames nobody is animating.
            _ => {
                let path = entry.gif.local_cache_path.as_deref().unwrap_or(&entry.gif.source_path);
                image(Handle::from_path(path))
                    .width(Length::Fixed(TILE_WIDTH))
                    .height(Length::Fixed(height))
                    .content_fit(ContentFit::Cover)
                    .border_radius(8.0)
                    .into()
            }
        };

        let on_press = if selection_mode {
            Message::ToggleTileSelected(gif_id)
        } else {
            Message::CopyGifFile(gif_id)
        };

        // A `button` (rather than `mouse_area`) so hovering gets a visible
        // highlight for free from iced's Hovered/Pressed status — an image
        // has no such feedback on its own.
        let clickable: Element<Message> = button(image_element)
            .on_press(on_press)
            .padding(0)
            .style(move |theme, status| {
                let accent = theme.extended_palette().primary.base.color;
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

                button::Style {
                    background: None,
                    text_color: Color::TRANSPARENT,
                    border: iced::Border {
                        color: if is_selected || hovered { accent } else { Color::TRANSPARENT },
                        width: if is_selected { 3.0 } else { 2.0 },
                        radius: 8.0.into(),
                    },
                    shadow: iced::Shadow::default(),
                    snap: false,
                }
            })
            .into();

        let mut layers = vec![clickable];

        if selection_mode {
            let mark = if is_selected { "✓" } else { "" };
            layers.push(
                container(
                    container(text(mark).size(14))
                        .width(Length::Fixed(22.0))
                        .height(Length::Fixed(22.0))
                        .align_x(Center)
                        .align_y(Center)
                        .style(move |theme: &Theme| {
                            let accent = theme.extended_palette().primary.base.color;
                            container::Style {
                                background: Some(if is_selected {
                                    accent.into()
                                } else {
                                    Color { a: 0.5, ..Color::BLACK }.into()
                                }),
                                border: iced::Border {
                                    color: Color::WHITE,
                                    width: 1.5,
                                    radius: 11.0.into(),
                                },
                                text_color: Some(Color::WHITE),
                                ..container::Style::default()
                            }
                        }),
                )
                .align_left(Length::Fixed(TILE_WIDTH))
                .align_top(Length::Fixed(height))
                .padding(6)
                .into(),
            );
        } else {
            let menu_button = container(
                button(text("⋮").size(18))
                    .on_press(Message::ShowDetail(gif_id))
                    .padding(4)
                    .style(button::secondary),
            )
            .align_right(Length::Fixed(TILE_WIDTH))
            .align_top(Length::Fixed(height))
            .padding(6);

            let source_badge = container(
                container(text(Self::source_icon(entry)).size(14))
                    .padding(4)
                    .style(container::rounded_box),
            )
            .align_left(Length::Fixed(TILE_WIDTH))
            .align_bottom(Length::Fixed(height))
            .padding(6);

            layers.push(menu_button.into());
            layers.push(source_badge.into());
        }

        stack(layers).into()
    }

    fn view(&self) -> Element<Message> {
        let page = match self.view_mode {
            ViewMode::Library => self.library_view(),
            ViewMode::Trash => self.trash_view(),
            ViewMode::Stats => self.stats_view(),
            ViewMode::Settings => self.settings_view(),
        };

        let base = row![self.sidebar_view(), page];

        let content: Element<Message> = if let Some(gif_id) = self.detail_gif_id {
            modal(base, self.detail_overlay_content(gif_id), Message::HideDetail)
        } else if let Some(gif_id) = self.pending_permanent_delete {
            modal(
                base,
                Self::permanent_delete_confirm_content(gif_id),
                Message::CancelPermanentDelete,
            )
        } else if let Some(path) = &self.pending_import_zip {
            modal(base, Self::import_confirm_content(path), Message::CancelImportBackup)
        } else if self.show_import_modal {
            modal(base, self.import_modal_content(), Message::HideImportModal)
        } else {
            base.into()
        };

        match &self.toast {
            Some((message, _)) => stack![content, Self::toast_overlay(message)].into(),
            None => content,
        }
    }

    fn sidebar_view(&self) -> Element<Message> {
        let nav_button = |label: &'static str, mode: ViewMode, active: bool| {
            button(text(label).size(14))
                .on_press(Message::SetViewMode(mode))
                .width(Length::Fill)
                .style(if active { button::primary } else { button::text })
        };

        let app_theme = self.app_theme;

        container(
            column![
                text("GifVault").size(22).font(Font::with_name("Fira Sans")),
                column![
                    nav_button("Library", ViewMode::Library, self.view_mode == ViewMode::Library),
                    nav_button("Trash", ViewMode::Trash, self.view_mode == ViewMode::Trash),
                    nav_button("Stats", ViewMode::Stats, self.view_mode == ViewMode::Stats),
                ]
                .spacing(4),
                container(column![]).height(Length::Fill),
                nav_button(
                    "Settings",
                    ViewMode::Settings,
                    self.view_mode == ViewMode::Settings
                ),
            ]
            .spacing(16)
            .padding(16)
            .width(Length::Fixed(170.0))
            .height(Length::Fill),
        )
        .style(move |_theme| container::Style {
            background: Some(app_theme.recessed_background().into()),
            ..container::Style::default()
        })
        .into()
    }

    fn library_view(&self) -> Element<Message> {
        let indices = self.filtered_indices();

        let grid: Element<Message> = if indices.is_empty() {
            text("No gifs match this filter").into()
        } else {
            let heights: Vec<f32> = indices
                .iter()
                .map(|&index| Self::tile_height_for(&self.entries[index]))
                .collect();
            let placements = compute_layout(&heights);

            let mut column_children: Vec<Vec<Element<Message>>> =
                (0..GRID_COLUMNS).map(|_| Vec::new()).collect();

            for (position, &entry_index) in indices.iter().enumerate() {
                let placement = &placements[position];
                let entry = &self.entries[entry_index];
                column_children[placement.column].push(Self::build_tile(
                    entry,
                    placement.height,
                    self.selection_mode,
                    self.selected_ids.contains(&entry.gif.id),
                ));
            }

            row(column_children
                .into_iter()
                .map(|children| column(children).spacing(TILE_SPACING).into()))
            .spacing(TILE_SPACING)
            .width(Length::Fill)
            .into()
        };

        let popular_tags = self.popular_tags();
        let popular_tags_row: Element<Message> = if popular_tags.is_empty() {
            column![].into()
        } else {
            row(popular_tags.into_iter().map(|tag| {
                button(text(tag).size(12))
                    .on_press(Message::PopularTagClicked(tag.to_string()))
                    .style(button::secondary)
                    .padding(6)
                    .into()
            }))
            .spacing(6)
            .into()
        };

        let sort_row = row(SortMode::ALL.iter().map(|&mode| {
            button(text(mode.label()).size(12))
                .on_press(Message::SortModeChanged(mode))
                .style(if self.sort_mode == mode {
                    button::primary
                } else {
                    button::secondary
                })
                .padding(6)
                .into()
        }))
        .spacing(6);

        let header = row![
            text("Library").size(26),
            container(column![]).width(Length::Fill),
            button(if self.selection_mode { "Cancel" } else { "Select" })
                .on_press(Message::ToggleSelectionMode)
                .style(if self.selection_mode { button::danger } else { button::secondary }),
        ]
        .align_y(Center);

        let selection_bar: Element<Message> = if self.selection_mode {
            let suggestion_hint: Element<Message> = match &self.tag_suggestion {
                Some(suggestion) => text(format!("Tab → {suggestion}")).size(12).into(),
                None => column![].into(),
            };

            container(
                column![
                    row![
                        text(format!("{} selected", self.selected_ids.len())).width(Length::Fill),
                        button("Move to trash")
                            .on_press(Message::BulkMoveToTrash)
                            .style(button::danger),
                    ]
                    .spacing(10)
                    .align_y(Center),
                    row![
                        text_input("Add a tag to all selected...", &self.tag_draft)
                            .id(TAG_DRAFT_INPUT_ID)
                            .on_input(Message::BulkTagDraftChanged)
                            .on_submit(Message::ApplyBulkTag)
                            .width(Length::Fill),
                        button("Add tag").on_press(Message::ApplyBulkTag),
                    ]
                    .spacing(10)
                    .align_y(Center),
                    suggestion_hint,
                ]
                .spacing(8),
            )
            .padding(12)
            .width(Length::Fill)
            .style(container::rounded_box)
            .into()
        } else {
            column![].into()
        };

        column![
            header,
            text_input("Filter by tag...", &self.filter_input)
                .on_input(Message::FilterChanged)
                .width(Length::Fill),
            popular_tags_row,
            sort_row,
            selection_bar,
            button("Add gif").on_press(Message::ShowImportModal),
            scrollable(grid)
                .on_scroll(Message::Scrolled)
                .height(Length::Fill)
                .width(Length::Fill),
        ]
        .spacing(10)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn trash_view(&self) -> Element<Message> {
        let body: Element<Message> = if self.deleted_entries.is_empty() {
            text("Trash is empty").into()
        } else {
            let rows: Vec<Element<Message>> = self
                .deleted_entries
                .iter()
                .map(|entry| {
                    let thumb: Element<Message> = match &entry.animation {
                        Some(animation) if !animation.frames.is_empty() => {
                            image(animation.frames[0].clone())
                                .width(Length::Fixed(70.0))
                                .height(Length::Fixed(70.0))
                                .content_fit(ContentFit::Cover)
                                .into()
                        }
                        _ => {
                            let path = entry
                                .gif
                                .local_cache_path
                                .as_deref()
                                .unwrap_or(&entry.gif.source_path);
                            image(Handle::from_path(path))
                                .width(Length::Fixed(70.0))
                                .height(Length::Fixed(70.0))
                                .content_fit(ContentFit::Cover)
                                .into()
                        }
                    };

                    let tags_line = if entry.tags.is_empty() {
                        "Tags: (none)".to_string()
                    } else {
                        format!("Tags: {}", entry.tags.join(", "))
                    };

                    let deleted_line = format!(
                        "Deleted: {}",
                        entry.gif.deleted_at.as_deref().unwrap_or("unknown")
                    );

                    container(
                        row![
                            thumb,
                            column![text(tags_line), text(deleted_line).size(12)]
                                .spacing(4)
                                .width(Length::Fill),
                            button("Restore").on_press(Message::RestoreGif(entry.gif.id)),
                            button("Delete forever")
                                .on_press(Message::RequestPermanentDelete(entry.gif.id))
                                .style(button::danger),
                        ]
                        .spacing(12)
                        .align_y(Center),
                    )
                    .padding(10)
                    .style(container::rounded_box)
                    .into()
                })
                .collect();

            scrollable(column(rows).spacing(10)).height(Length::Fill).into()
        };

        column![
            row![
                text("Trash").size(26),
                container(column![]).width(Length::Fill),
                button("Empty trash").on_press(Message::EmptyTrash).style(button::danger),
            ]
            .align_y(Center),
            body,
        ]
        .spacing(16)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn settings_view(&self) -> Element<Message> {
        let tag_rows: Vec<Element<Message>> = self
            .tag_stats
            .iter()
            .map(|(name, count)| {
                if self.renaming_tag.as_deref() == Some(name.as_str()) {
                    row![
                        text_input("New name...", &self.tag_rename_draft)
                            .on_input(Message::TagRenameDraftChanged)
                            .on_submit(Message::ConfirmRenameTag)
                            .width(Length::Fill),
                        button("Save").on_press(Message::ConfirmRenameTag),
                        button("Cancel")
                            .on_press(Message::CancelRenameTag)
                            .style(button::text),
                    ]
                    .spacing(6)
                    .align_y(Center)
                    .into()
                } else {
                    row![
                        text(format!("{name} ({count})")).width(Length::Fill),
                        button("Rename")
                            .on_press(Message::StartRenameTag(name.clone()))
                            .style(button::text),
                    ]
                    .spacing(6)
                    .align_y(Center)
                    .into()
                }
            })
            .collect();

        let tags_section: Element<Message> = if self.tag_stats.is_empty() {
            column![].into()
        } else {
            column![
                text("Manage tags").size(14),
                text("Rename a tag to an existing name to merge the two.").size(12),
                column(tag_rows).spacing(8),
            ]
            .spacing(8)
            .into()
        };

        let content = column![
            text("Settings").size(26),
            column![
                text("Theme").size(14),
                row![
                    button("Light")
                        .on_press(Message::ThemeChanged(AppTheme::Light))
                        .style(if self.app_theme == AppTheme::Light {
                            button::primary
                        } else {
                            button::secondary
                        }),
                    button("Dark")
                        .on_press(Message::ThemeChanged(AppTheme::Dark))
                        .style(if self.app_theme == AppTheme::Dark {
                            button::primary
                        } else {
                            button::secondary
                        }),
                ]
                .spacing(10),
            ]
            .spacing(8),
            column![
                text("Trash").size(14),
                button("Empty trash").on_press(Message::EmptyTrash).style(button::danger),
            ]
            .spacing(8),
            column![
                text("Backup").size(14),
                row![
                    button("Export library").on_press(Message::ExportBackup),
                    button("Import from zip").on_press(Message::ImportBackupClicked),
                ]
                .spacing(10),
                text("Importing replaces your current library with the zip's contents.").size(12),
            ]
            .spacing(8),
            tags_section,
        ]
        .spacing(24)
        .padding(20)
        .width(Length::Fill);

        scrollable(content).height(Length::Fill).width(Length::Fill).into()
    }

    fn stats_view(&self) -> Element<Message> {
        let stat_row = |label: &'static str, value: String| -> Element<'static, Message> {
            row![text(label).width(Length::Fill), text(value)].into()
        };

        let rows = column![
            stat_row("Gifs in library", self.stats.gif_count.to_string()),
            stat_row("Total size on disk", Self::format_bytes(self.stats.total_size_bytes)),
            stat_row("Imported from a folder", self.stats.local_count.to_string()),
            stat_row("Imported from a URL", self.stats.url_count.to_string()),
            stat_row("Clipboard copies (all time)", self.stats.total_copies.to_string()),
            stat_row("Items in trash", self.stats.trash_count.to_string()),
            stat_row("Reclaimable if trash is emptied", Self::format_bytes(self.stats.trash_size_bytes)),
        ]
        .spacing(12);

        column![
            text("Stats").size(26),
            container(rows).padding(16).style(container::rounded_box),
        ]
        .spacing(16)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn import_confirm_content(path: &Path) -> Element<'static, Message> {
        container(
            column![
                text("Replace current library?").size(18),
                text(format!(
                    "This will overwrite your current gifs, tags and settings with the \
                     contents of:\n{}",
                    path.display()
                ))
                .size(13),
                row![
                    button("Cancel").on_press(Message::CancelImportBackup),
                    button("Replace library")
                        .on_press(Message::ConfirmImportBackup)
                        .style(button::danger),
                ]
                .spacing(10),
            ]
            .spacing(12),
        )
        .padding(20)
        .width(420)
        .style(container::rounded_box)
        .into()
    }

    fn permanent_delete_confirm_content(gif_id: i64) -> Element<'static, Message> {
        container(
            column![
                text("Delete this gif forever?").size(18),
                text("This removes the file from disk. It can't be undone.").size(13),
                row![
                    button("Cancel").on_press(Message::CancelPermanentDelete),
                    button("Delete forever")
                        .on_press(Message::PermanentlyDeleteGif(gif_id))
                        .style(button::danger),
                ]
                .spacing(10),
            ]
            .spacing(12),
        )
        .padding(20)
        .width(360)
        .style(container::rounded_box)
        .into()
    }

    /// Shows just the start of a (possibly very long) source link plus a
    /// link icon, with the full value available on hover — the add-gif
    /// screen doesn't need to be cluttered with a whole URL or file path.
    fn truncated_source_label(source: &str) -> Element<'static, Message> {
        const PREVIEW_CHARS: usize = 40;

        let short = if source.chars().count() > PREVIEW_CHARS {
            let prefix: String = source.chars().take(PREVIEW_CHARS).collect();
            format!("{prefix}… 🔗")
        } else {
            format!("{source} 🔗")
        };

        tooltip(
            text(short).size(13),
            container(text(source.to_string()).size(13))
                .padding(8)
                .style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn toast_overlay(message: &str) -> Element<'_, Message> {
        container(
            container(text(message.to_string()))
                .padding(10)
                .style(container::rounded_box),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Center)
        .align_y(Bottom)
        .padding(30)
        .into()
    }

    /// Lays tag pills out left-to-right, wrapping to a new line once a row
    /// would overflow the modal's width (iced has no built-in wrap layout).
    fn wrap_tag_pills(staged_tags: &[String]) -> Element<'_, Message> {
        const MAX_ROW_WIDTH: f32 = 400.0;
        const CHAR_WIDTH: f32 = 7.0;
        const CHIP_OVERHEAD: f32 = 46.0;

        let mut lines: Vec<Vec<Element<Message>>> = vec![Vec::new()];
        let mut current_width = 0.0;

        for (index, tag) in staged_tags.iter().enumerate() {
            let chip_width = tag.chars().count() as f32 * CHAR_WIDTH + CHIP_OVERHEAD;

            if current_width + chip_width > MAX_ROW_WIDTH && !lines.last().unwrap().is_empty() {
                lines.push(Vec::new());
                current_width = 0.0;
            }

            current_width += chip_width + 6.0;
            lines.last_mut().unwrap().push(Self::tag_pill(index, tag));
        }

        column(lines.into_iter().map(|line| row(line).spacing(6).into()))
            .spacing(6)
            .into()
    }

    fn tag_pill(index: usize, tag: &str) -> Element<'static, Message> {
        container(
            row![
                text(tag.to_string()).size(13),
                button(text("×").size(13))
                    .on_press(Message::RemoveStagedTag(index))
                    .padding(2)
                    .style(button::text),
            ]
            .spacing(4)
            .align_y(Center),
        )
        .padding(6)
        .style(container::rounded_box)
        .into()
    }

    fn detail_overlay_content(&self, gif_id: i64) -> Element<Message> {
        let Some(entry) = self.entries.iter().find(|entry| entry.gif.id == gif_id) else {
            return column![].into();
        };

        let preview: Element<Message> = match &entry.animation {
            Some(animation) if !animation.frames.is_empty() => {
                let frame_index = entry.current_frame.min(animation.frames.len() - 1);
                image(animation.frames[frame_index].clone())
                    .width(Length::Fixed(400.0))
                    .content_fit(ContentFit::Contain)
                    .into()
            }
            _ => {
                let path = entry.gif.local_cache_path.as_deref().unwrap_or(&entry.gif.source_path);
                image(Handle::from_path(path))
                    .width(Length::Fixed(400.0))
                    .content_fit(ContentFit::Contain)
                    .into()
            }
        };

        let tags_section: Element<Message> = if self.editing_tags_for == Some(gif_id) {
            let suggestion_hint: Element<Message> = match &self.tag_suggestion {
                Some(suggestion) => text(format!("Tab → {suggestion}")).size(12).into(),
                None => column![].into(),
            };

            column![
                text("Tags").size(13),
                Self::wrap_tag_pills(&self.staged_tags),
                text_input("Type a tag, Enter or comma to add...", &self.tag_draft)
                    .id(TAG_DRAFT_INPUT_ID)
                    .on_input(Message::TagDraftChanged)
                    .on_submit(Message::CommitTagDraft)
                    .width(Length::Fill),
                suggestion_hint,
                row![
                    button("Cancel").on_press(Message::CancelTagsEdit),
                    button("Save tags").on_press(Message::SaveTagsEdit(gif_id)),
                ]
                .spacing(10),
            ]
            .spacing(6)
            .into()
        } else {
            let tags_line = if entry.tags.is_empty() {
                "Tags: (none)".to_string()
            } else {
                format!("Tags: {}", entry.tags.join(", "))
            };

            row![
                text(tags_line).width(Length::Fill),
                button("Edit tags").on_press(Message::EditTagsClicked(gif_id)),
            ]
            .spacing(8)
            .align_y(Center)
            .into()
        };

        column![
            preview,
            text(format!("Added: {}", entry.gif.added_at)),
            tags_section,
            row![
                text(Self::source_icon(entry)).size(16),
                button("Copy file").on_press(Message::CopyGifFile(gif_id)),
            ]
            .spacing(8)
            .align_y(Center),
            row![
                button("Move to trash")
                    .on_press(Message::MoveToTrash(gif_id))
                    .style(button::danger),
                button("Close").on_press(Message::HideDetail),
            ]
            .spacing(10),
        ]
        .spacing(12)
        .into()
    }

    fn import_modal_content(&self) -> Element<Message> {
        let has_staged_content = self.pending_file.is_some() || self.staged_gif.is_some();

        let content: Element<Message> = if has_staged_content {
            let preview: Element<Message> = match &self.pending_thumbnail {
                Some(handle) => image(handle.clone()).width(200).height(200).into(),
                None => text("(preview unavailable)").into(),
            };

            let source_label = match (&self.pending_file, &self.staged_gif) {
                (Some(path), _) => path.to_string_lossy().to_string(),
                (_, Some(staged)) => staged.source_path.clone(),
                (None, None) => String::new(),
            };

            // The Copy/Move choice only makes sense for local files;
            // a downloaded gif is already stored, there is nothing to move.
            let mode_row: Element<Message> = if self.pending_file.is_some() {
                row![
                    button("Copy")
                        .on_press(Message::ImportModeChanged(ImportMode::Copy))
                        .style(if self.import_mode == ImportMode::Copy {
                            button::primary
                        } else {
                            button::secondary
                        }),
                    button("Move")
                        .on_press(Message::ImportModeChanged(ImportMode::Move))
                        .style(if self.import_mode == ImportMode::Move {
                            button::primary
                        } else {
                            button::secondary
                        }),
                ]
                .spacing(10)
                .into()
            } else {
                column![].into()
            };

            let error_text: Element<Message> = match &self.import_error {
                Some(err) => text(err).color(Color::from_rgb(0.8, 0.2, 0.2)).into(),
                None => column![].into(),
            };

            let suggestion_hint: Element<Message> = match &self.tag_suggestion {
                Some(suggestion) => text(format!("Tab → {suggestion}")).size(12).into(),
                None => column![].into(),
            };

            let tags_section = column![
                Self::wrap_tag_pills(&self.staged_tags),
                text_input("Type a tag, Enter or comma to add...", &self.tag_draft)
                    .id(TAG_DRAFT_INPUT_ID)
                    .on_input(Message::TagDraftChanged)
                    .on_submit(Message::CommitTagDraft)
                    .width(Length::Fill),
                suggestion_hint,
            ]
            .spacing(6);

            column![
                preview,
                Self::truncated_source_label(&source_label),
                mode_row,
                tags_section,
                error_text,
                row![
                    button("Cancel").on_press(Message::HideImportModal),
                    button("Add").on_press(Message::ConfirmImport),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .into()
        } else {
            let fetch_button = if self.is_fetching_url {
                button("Fetching...")
            } else {
                button("Fetch from URL").on_press(Message::FetchUrl)
            };

            let error_text: Element<Message> = match &self.import_error {
                Some(err) => text(err).color(Color::from_rgb(0.8, 0.2, 0.2)).into(),
                None => column![].into(),
            };

            column![
                text("No file selected"),
                button("Choose file").on_press(Message::PickFile),
                text("— or —"),
                text_input("Paste a GIF URL...", &self.url_input)
                    .on_input(Message::UrlInputChanged)
                    .width(Length::Fill),
                fetch_button,
                error_text,
                button("Cancel").on_press(Message::HideImportModal),
            ]
            .spacing(10)
            .into()
        };

        container(content)
            .padding(20)
            .width(450)
            .style(container::rounded_box)
            .into()
    }
}

fn modal<'a>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
) -> Element<'a, Message> {
    stack![
        base.into(),
        opaque(
            mouse_area(center(opaque(content)).style(|_theme| container::Style {
                background: Some(
                    Color {
                        a: 0.8,
                        ..Color::BLACK
                    }
                    .into()
                ),
                ..container::Style::default()
            }))
            .on_press(on_blur)
        )
    ]
    .into()
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("GifVault")
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(Font::with_name("Fira Sans"))
        .window(WindowSettings {
            size: Size::new(1000.0, 700.0),
            ..WindowSettings::default()
        })
        .run()
}
