use crate::model::Gif;
use rusqlite::{Connection, Result};

pub fn insert_gif(
    conn: &Connection,
    source_type: &str,
    source_path: &str,
    local_cache_path: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO gifs (source_type, source_path, local_cache_path, added_at)
         VALUES (?1, ?2, ?3, datetime('now'))",
        (source_type, source_path, local_cache_path),
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn set_tags_for_gif(conn: &Connection, gif_id: i64, tags_input: &str) -> Result<()> {
    for raw_tag in tags_input.split(',') {
        let tag = raw_tag.trim();
        if tag.is_empty() {
            continue;
        }

        conn.execute(
            "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
            [tag],
        )?;

        let tag_id: i64 = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            [tag],
            |row| row.get(0),
        )?;

        conn.execute(
            "INSERT INTO gif_tags (gif_id, tag_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            (gif_id, tag_id),
        )?;
    }

    Ok(())
}

/// Replaces a gif's tags with exactly the given set — unlike
/// `set_tags_for_gif`, this also removes associations for tags that were
/// unstaged, so it's suitable for editing an existing gif's tags rather
/// than only ever adding to them.
pub fn set_gif_tags_exact(conn: &Connection, gif_id: i64, tags: &[String]) -> Result<()> {
    conn.execute("DELETE FROM gif_tags WHERE gif_id = ?1", [gif_id])?;
    set_tags_for_gif(conn, gif_id, &tags.join(","))
}

/// Renames a tag. If `new_name` already exists, the two are merged instead:
/// every gif tagged with `old_name` ends up tagged with the existing
/// `new_name` tag, and the now-empty `old_name` row is removed.
pub fn rename_or_merge_tag(conn: &Connection, old_name: &str, new_name: &str) -> Result<()> {
    let old_id: i64 =
        conn.query_row("SELECT id FROM tags WHERE name = ?1", [old_name], |row| row.get(0))?;

    let existing_target: Option<i64> = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", [new_name], |row| row.get(0))
        .ok();

    match existing_target {
        None => {
            conn.execute("UPDATE tags SET name = ?1 WHERE id = ?2", (new_name, old_id))?;
        }
        Some(target_id) if target_id == old_id => {
            // Renaming a tag to the name it already has: nothing to do.
        }
        Some(target_id) => {
            // Reassign what we can; any gif that already has both ends up
            // with a duplicate (gif_id, tag_id) pair, which the unique
            // primary key rejects — those just get dropped instead below.
            conn.execute(
                "UPDATE OR IGNORE gif_tags SET tag_id = ?1 WHERE tag_id = ?2",
                (target_id, old_id),
            )?;
            conn.execute("DELETE FROM gif_tags WHERE tag_id = ?1", [old_id])?;
            conn.execute("DELETE FROM tags WHERE id = ?1", [old_id])?;
        }
    }

    Ok(())
}

/// Irreversibly removes a gif's row (and its tag links). Only meant to be
/// called from the trash view, on a gif that is already soft-deleted.
pub fn permanently_delete_gif(conn: &Connection, gif_id: i64) -> Result<()> {
    conn.execute("DELETE FROM gif_tags WHERE gif_id = ?1", [gif_id])?;
    conn.execute("DELETE FROM gifs WHERE id = ?1", [gif_id])?;

    Ok(())
}

/// Moves a gif to the trash without touching its file on disk, so it can
/// still be restored later.
pub fn soft_delete_gif(conn: &Connection, gif_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE gifs SET deleted_at = datetime('now') WHERE id = ?1",
        [gif_id],
    )?;

    Ok(())
}

pub fn restore_gif(conn: &Connection, gif_id: i64) -> Result<()> {
    conn.execute("UPDATE gifs SET deleted_at = NULL WHERE id = ?1", [gif_id])?;

    Ok(())
}

pub fn increment_use_count(conn: &Connection, gif_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE gifs SET use_count = use_count + 1 WHERE id = ?1",
        [gif_id],
    )?;

    Ok(())
}

const GIF_COLUMNS: &str = "id, source_type, source_path, local_cache_path, title, added_at, \
     file_hash, deleted_at, use_count, width, height";

fn row_to_gif(row: &rusqlite::Row) -> Result<Gif> {
    Ok(Gif {
        id: row.get(0)?,
        source_type: row.get(1)?,
        source_path: row.get(2)?,
        local_cache_path: row.get(3)?,
        title: row.get(4)?,
        added_at: row.get(5)?,
        file_hash: row.get(6)?,
        deleted_at: row.get(7)?,
        use_count: row.get(8)?,
        width: row.get(9)?,
        height: row.get(10)?,
    })
}

/// Records a gif's pixel dimensions, peeked from its header at import time
/// (or backfilled once for gifs imported before this column existed) so the
/// grid can size its tile without ever decoding its frames.
pub fn set_gif_dimensions(conn: &Connection, gif_id: i64, width: u32, height: u32) -> Result<()> {
    conn.execute(
        "UPDATE gifs SET width = ?1, height = ?2 WHERE id = ?3",
        (width, height, gif_id),
    )?;

    Ok(())
}

pub fn list_gifs(conn: &Connection) -> Result<Vec<Gif>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {GIF_COLUMNS} FROM gifs WHERE deleted_at IS NULL ORDER BY added_at DESC"
    ))?;

    let rows = stmt.query_map([], row_to_gif)?;

    rows.collect()
}

pub fn list_deleted_gifs(conn: &Connection) -> Result<Vec<Gif>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {GIF_COLUMNS} FROM gifs WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
    ))?;

    let rows = stmt.query_map([], row_to_gif)?;

    rows.collect()
}

/// Fetches a single gif's row, e.g. right after inserting or soft-deleting
/// it, so the caller can update its in-memory copy without re-reading (and
/// re-decoding the gif frames for) the whole library.
pub fn get_gif(conn: &Connection, gif_id: i64) -> Result<Gif> {
    conn.query_row(
        &format!("SELECT {GIF_COLUMNS} FROM gifs WHERE id = ?1"),
        [gif_id],
        row_to_gif,
    )
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    match conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(other) => Err(other),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (key, value),
    )?;

    Ok(())
}

/// Every tag with how many gifs currently use it, including unused tags
/// (count 0). Powers both the "popular tags" quick filters and the tag
/// autocomplete when adding a gif.
pub fn list_tag_stats(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT tags.name, COUNT(gif_tags.gif_id) AS usage_count
         FROM tags
         LEFT JOIN gif_tags ON gif_tags.tag_id = tags.id
         GROUP BY tags.id
         ORDER BY tags.name",
    )?;

    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;

    rows.collect()
}

pub fn list_tags_for_gif(conn: &Connection, gif_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT tags.name
         FROM tags
         JOIN gif_tags ON gif_tags.tag_id = tags.id
         WHERE gif_tags.gif_id = ?1
         ORDER BY tags.name",
    )?;

    let rows = stmt.query_map([gif_id], |row| row.get(0))?;

    rows.collect()
}