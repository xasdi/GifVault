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

struct GifEntry {
    gif: Gif,
    thumbnail: Option<Handle>,
    tags: Vec<String>,
}

struct App {
    conn: Connection,
    entries: Vec<GifEntry>,
    show_import_modal: bool,
    pending_file: Option<PathBuf>,
    pending_thumbnail: Option<Handle>,
    modal_tags_input: String,
}

#[derive(Debug, Clone)]
enum Message {
    ShowImportModal,
    HideImportModal,
    PickFile,
    FileSelected(Option<PathBuf>),
    ModalTagsChanged(String),
    ConfirmImport,
}

impl App {
    fn new() -> Self {
        let conn = db::init_db();
        let entries = Self::load_entries(&conn);

        Self {
            conn,
            entries,
            show_import_modal: false,
            pending_file: None,
            pending_thumbnail: None,
            modal_tags_input: String::new(),
        }
    }

    fn load_entries(conn: &Connection) -> Vec<GifEntry> {
        let gifs = db::queries::list_gifs(conn).unwrap_or_default();

        gifs.into_iter()
            .map(|gif| {
                let thumbnail = thumbnail::load_thumbnail(&gif.source_path);
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
        self.pending_thumbnail = None;
        self.modal_tags_input.clear();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ShowImportModal => {
                self.show_import_modal = true;
                self.pending_file = None;
                self.pending_thumbnail = None;
                self.modal_tags_input.clear();
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
                }
            }
            Message::ModalTagsChanged(value) => {
                self.modal_tags_input = value;
            }
            Message::ConfirmImport => {
                if let Some(path) = &self.pending_file {
                    let path_str = path.to_string_lossy().to_string();

                    let gif_id = db::queries::insert_gif(&self.conn, "local", &path_str)
                        .expect("could not insert gif");

                    db::queries::set_tags_for_gif(&self.conn, gif_id, &self.modal_tags_input)
                        .expect("could not save tags");

                    self.entries = Self::load_entries(&self.conn);
                    self.reset_modal_state();
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let gif_list = self.entries.iter().fold(column![], |col, entry| {
            let tags_line = if entry.tags.is_empty() {
                "Tags: (none)".to_string()
            } else {
                format!("Tags: {}", entry.tags.join(", "))
            };

            let info = column![text(&entry.gif.source_path), text(tags_line)].width(Length::Fill);

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
        .spacing(15);

        let base = column![
            text("GifVault"),
            text(format!("Gifs in database: {}", self.entries.len())),
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
        let content: Element<Message> = match (&self.pending_file, &self.pending_thumbnail) {
            (Some(path), thumbnail) => {
                let preview: Element<Message> = match thumbnail {
                    Some(handle) => image(handle.clone()).width(200).height(200).into(),
                    None => text("(preview unavailable)").into(),
                };

                column![
                    preview,
                    text(path.to_string_lossy().to_string()),
                    text_input("Tags (comma separated)", &self.modal_tags_input)
                        .on_input(Message::ModalTagsChanged)
                        .width(Length::Fill),
                    row![
                        button("Cancel").on_press(Message::HideImportModal),
                        button("Add").on_press(Message::ConfirmImport),
                    ]
                    .spacing(10),
                ]
                .spacing(10)
                .into()
            }
            (None, _) => column![
                text("No file selected"),
                row![
                    button("Cancel").on_press(Message::HideImportModal),
                    button("Choose file").on_press(Message::PickFile),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .into(),
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