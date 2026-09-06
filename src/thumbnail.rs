use iced::widget::image::Handle;

pub fn load_thumbnail(path: &str) -> Option<Handle> {
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    Some(Handle::from_rgba(width, height, rgba.into_raw()))
}