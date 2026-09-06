mod db;
mod model;
mod thumbnail;

use iced::widget::image::Handle;
use iced::widget::{
    button, center, column, container, image, mouse_area, opaque, row, scrollable, stack, text,
    text_input,
};
use iced::window::Settings as WindowSettings;
use iced::{Color, Element, Length, Size, Task};
use model::Gif;
use rusqlite::Connection;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
enum ImportMode {
    Copy,
    Move,
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
    thumbnail: Option<Handle>,
    tags: Vec<String>,
}

struct App {
    conn: Connection,
    entries: Vec<GifEntry>,
    filter_input: String,
    show_import_modal: bool,
    pending_file: Option<PathBuf>,
    staged_gif: Option<StagedGif>,
    pending_thumbnail: Option<Handle>,
    modal_tags_input: String,
    import_mode: ImportMode,
    url_input: String,
    is_fetching_url: bool,
    import_error: Option<String>,
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
    ModalTagsChanged(String),
    ImportModeChanged(ImportMode),
    ConfirmImport,
}

impl App {
    fn new() -> Self {
        let conn = db::init_db();
        let entries = Self::load_entries(&conn);

        Self {
            conn,
            entries,
            filter_input: String::new(),
            show_import_modal: false,
            pending_file: None,
            staged_gif: None,
            pending_thumbnail: None,
            modal_tags_input: String::new(),
            import_mode: ImportMode::Copy,
            url_input: String::new(),
            is_fetching_url: false,
            import_error: None,
        }
    }

    fn load_entries(conn: &Connection) -> Vec<GifEntry> {
        let gifs = db::queries::list_gifs(conn).unwrap_or_default();

        gifs.into_iter()
            .map(|gif| {
                let path_to_load = gif.local_cache_path.as_deref().unwrap_or(&gif.source_path);
                let thumbnail = thumbnail::load_thumbnail(path_to_load);
                let tags = db::queries::list_tags_for_gif(conn, gif.id).unwrap_or_default();
                GifEntry {
                    gif,
                    thumbnail,
                    tags,
                }
            })
            .collect()
    }

    fn reset_modal_state(&mut self) {
        self.show_import_modal = false;
        self.pending_file = None;
        self.staged_gif = None;
        self.pending_thumbnail = None;
        self.modal_tags_input.clear();
        self.import_mode = ImportMode::Copy;
        self.url_input.clear();
        self.is_fetching_url = false;
        self.import_error = None;
    }

    fn filtered_entries(&self) -> Vec<&GifEntry> {
        let filter = self.filter_input.trim().to_lowercase();

        if filter.is_empty() {
            return self.entries.iter().collect();
        }

        self.entries
            .iter()
            .filter(|entry| {
                entry
                    .tags
                    .iter()
                    .any(|tag| tag.to_lowercase().contains(&filter))
            })
            .collect()
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

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FilterChanged(value) => {
                self.filter_input = value;
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
                    self.pending_thumbnail = thumbnail::load_thumbnail(&path_str);
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
                        self.pending_thumbnail = thumbnail::load_thumbnail(&stored_path_str);
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
            Message::ModalTagsChanged(value) => {
                self.modal_tags_input = value;
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

                    db::queries::set_tags_for_gif(&self.conn, gif_id, &self.modal_tags_input)
                        .expect("could not save tags");

                    self.entries = Self::load_entries(&self.conn);
                    self.reset_modal_state();
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
                                &self.modal_tags_input,
                            )
                            .expect("could not save tags");

                            self.entries = Self::load_entries(&self.conn);
                            self.reset_modal_state();
                        }
                        Err(err) => {
                            self.import_error = Some(err);
                        }
                    }
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let visible_entries = self.filtered_entries();

        let gif_list: Element<Message> = if visible_entries.is_empty() {
            text("No gifs match this filter").into()
        } else {
            visible_entries
                .into_iter()
                .fold(column![], |col, entry| {
                    let tags_line = if entry.tags.is_empty() {
                        "Tags: (none)".to_string()
                    } else {
                        format!("Tags: {}", entry.tags.join(", "))
                    };

                    let info =
                        column![text(&entry.gif.source_path), text(tags_line)].width(Length::Fill);

                    let row_element: Element<Message> = match &entry.thumbnail {
                        Some(handle) => row![image(handle.clone()).width(80).height(80), info]
                            .spacing(10)
                            .width(Length::Fill)
                            .into(),
                        None => row![text("(preview unavailable)"), info]
                            .spacing(10)
                            .width(Length::Fill)
                            .into(),
                    };

                    col.push(row_element)
                })
                .width(Length::Fill)
                .spacing(15)
                .into()
        };

        let base = column![
            text("GifVault"),
            text(format!("Gifs in database: {}", self.entries.len())),
            text_input("Filter by tag...", &self.filter_input)
                .on_input(Message::FilterChanged)
                .width(Length::Fill),
            button("Add gif").on_press(Message::ShowImportModal),
            scrollable(gif_list).height(Length::Fill).width(Length::Fill),
        ]
        .spacing(10)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill);

        if self.show_import_modal {
            modal(base, self.import_modal_content(), Message::HideImportModal)
        } else {
            base.into()
        }
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

            column![
                preview,
                text(source_label),
                mode_row,
                text_input("Tags (comma separated)", &self.modal_tags_input)
                    .on_input(Message::ModalTagsChanged)
                    .width(Length::Fill),
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
        .window(WindowSettings {
            size: Size::new(1000.0, 700.0),
            ..WindowSettings::default()
        })
        .run()
}