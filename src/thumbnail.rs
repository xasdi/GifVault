use iced::widget::image::Handle;
use image::{codecs::gif::GifDecoder, imageops, AnimationDecoder, ImageDecoder, ImageReader, RgbaImage};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

/// A tile never renders larger than a few hundred pixels wide, so frames
/// are downscaled to this on decode — keeps memory bounded regardless of
/// the source gif's actual resolution.
const MAX_FRAME_DIMENSION: u32 = 480;

/// Bumped whenever the on-disk cache format changes, so old cache files are
/// transparently ignored (and rewritten) instead of misread.
const CACHE_MAGIC: u32 = 0x4756_4301;

/// Decoded frames of a gif (or a single-frame fallback for non-gif images),
/// ready to be played back in a widget.
pub struct GifAnimation {
    pub frames: Vec<Handle>,
    pub delays_ms: Vec<u64>,
}

/// The same decoded data as [`GifAnimation`], but as raw pixel bytes instead
/// of iced `Handle`s — this is the form that gets written to / read from the
/// on-disk cache, and `Handle`s are cheaply built from it on demand.
struct RawAnimation {
    width: u32,
    height: u32,
    frames: Vec<(u64, Vec<u8>)>,
}

impl RawAnimation {
    fn into_gif_animation(self) -> GifAnimation {
        let width = self.width;
        let height = self.height;
        let mut frames = Vec::with_capacity(self.frames.len());
        let mut delays_ms = Vec::with_capacity(self.frames.len());

        for (delay_ms, pixels) in self.frames {
            delays_ms.push(delay_ms);
            frames.push(Handle::from_rgba(width, height, pixels));
        }

        GifAnimation { frames, delays_ms }
    }
}

/// Peeks a gif's (or any supported image's) pixel dimensions straight from
/// its header, without decoding any frame data. Cheap enough to call at
/// import time so the grid knows how to size a tile long before it's ever
/// actually decoded.
pub fn peek_dimensions(path: &str) -> Option<(u32, u32)> {
    if let Ok(file) = File::open(path) {
        if let Ok(decoder) = GifDecoder::new(BufReader::new(file)) {
            return Some(decoder.dimensions());
        }
    }

    ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Decodes straight from the source file, bypassing the cache. Used when
/// importing a gif for the first time — there is nothing to cache yet.
pub fn load_animation(path: &str) -> Option<GifAnimation> {
    decode_raw(path).map(RawAnimation::into_gif_animation)
}

/// Loads a gif's animation, transparently reusing an already-decoded copy
/// from `cache_dir` when one exists, and writing one after a fresh decode.
/// Repeat views of the same gif — including across app restarts — skip
/// decoding the source file entirely.
pub fn load_animation_cached(gif_id: i64, source_path: &str, cache_dir: &Path) -> Option<GifAnimation> {
    let cache_path = cache_dir.join(format!("{gif_id}.cache"));

    if let Some(raw) = read_cache(&cache_path) {
        return Some(raw.into_gif_animation());
    }

    let raw = decode_raw(source_path)?;
    if let Err(err) = write_cache(&cache_path, &raw) {
        eprintln!("Failed to write decode cache for gif {gif_id}: {err}");
    }
    Some(raw.into_gif_animation())
}

/// Deletes a gif's cached decode, if any. Meant to be called when the gif
/// itself is permanently deleted, so the cache doesn't accumulate forever.
pub fn remove_cache(gif_id: i64, cache_dir: &Path) {
    let _ = std::fs::remove_file(cache_dir.join(format!("{gif_id}.cache")));
}

fn decode_raw(path: &str) -> Option<RawAnimation> {
    decode_gif_raw(path).or_else(|| decode_static_raw(path))
}

fn decode_gif_raw(path: &str) -> Option<RawAnimation> {
    let file = File::open(path).ok()?;
    let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
    let frames = decoder.into_frames().collect_frames().ok()?;

    if frames.is_empty() {
        return None;
    }

    let (width, height) = downscaled_dimensions(frames[0].buffer().dimensions());
    let mut raw_frames = Vec::with_capacity(frames.len());

    for frame in frames {
        let (numer, denom) = frame.delay().numer_denom_ms();
        // Some gifs encode a delay of 0, which viewers conventionally treat
        // as "default speed" rather than literally instant.
        let delay_ms = if denom == 0 { 100 } else { numer / denom };
        let buffer = downscale_if_needed(frame.into_buffer());
        raw_frames.push((delay_ms.max(20) as u64, buffer.into_raw()));
    }

    Some(RawAnimation { width, height, frames: raw_frames })
}

fn decode_static_raw(path: &str) -> Option<RawAnimation> {
    let img = image::open(path).ok()?;
    let rgba = downscale_if_needed(img.to_rgba8());
    let (width, height) = rgba.dimensions();

    Some(RawAnimation { width, height, frames: vec![(100, rgba.into_raw())] })
}

fn downscaled_dimensions((width, height): (u32, u32)) -> (u32, u32) {
    let longest = width.max(height);

    if longest <= MAX_FRAME_DIMENSION {
        return (width, height);
    }

    let scale = MAX_FRAME_DIMENSION as f32 / longest as f32;
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

fn downscale_if_needed(rgba: RgbaImage) -> RgbaImage {
    let (new_width, new_height) = downscaled_dimensions(rgba.dimensions());

    if (new_width, new_height) == rgba.dimensions() {
        return rgba;
    }

    imageops::resize(&rgba, new_width, new_height, imageops::FilterType::Triangle)
}

fn read_cache(path: &Path) -> Option<RawAnimation> {
    let mut file = BufReader::new(File::open(path).ok()?);

    if read_u32(&mut file)? != CACHE_MAGIC {
        return None;
    }

    let width = read_u32(&mut file)?;
    let height = read_u32(&mut file)?;
    let frame_count = read_u32(&mut file)? as usize;

    if width == 0 || height == 0 || frame_count == 0 {
        return None;
    }

    let frame_size = width as usize * height as usize * 4;
    let mut frames = Vec::with_capacity(frame_count);

    for _ in 0..frame_count {
        let delay_ms = read_u64(&mut file)?;
        let mut pixels = vec![0u8; frame_size];
        file.read_exact(&mut pixels).ok()?;
        frames.push((delay_ms, pixels));
    }

    Some(RawAnimation { width, height, frames })
}

fn write_cache(path: &Path, animation: &RawAnimation) -> io::Result<()> {
    // Written to a temp file and renamed into place, so a decode that gets
    // interrupted mid-write can never leave a corrupt cache file behind.
    let tmp_path = path.with_extension("cache.tmp");
    {
        let mut file = BufWriter::new(File::create(&tmp_path)?);
        file.write_all(&CACHE_MAGIC.to_le_bytes())?;
        file.write_all(&animation.width.to_le_bytes())?;
        file.write_all(&animation.height.to_le_bytes())?;
        file.write_all(&(animation.frames.len() as u32).to_le_bytes())?;

        for (delay_ms, pixels) in &animation.frames {
            file.write_all(&delay_ms.to_le_bytes())?;
            file.write_all(pixels)?;
        }
    }

    std::fs::rename(&tmp_path, path)
}

fn read_u32(reader: &mut impl Read) -> Option<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).ok()?;
    Some(u32::from_le_bytes(buf))
}

fn read_u64(reader: &mut impl Read) -> Option<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf).ok()?;
    Some(u64::from_le_bytes(buf))
}
