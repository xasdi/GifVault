use crate::model::Gif;
use rusqlite::{Connection, Result};

pub fn insert_gif(conn: &Connection, source_type: &str, source_path: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO gifs (source_type, source_path, added_at) VALUES (?1, ?2, datetime('now'))",
        (source_type, source_path),
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

pub fn list_gifs(conn: &Connection) -> Result<Vec<Gif>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_type, source_path, local_cache_path, title, added_at, file_hash
         FROM gifs
         ORDER BY added_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(Gif {
            id: row.get(0)?,
            source_type: row.get(1)?,
            source_path: row.get(2)?,
            local_cache_path: row.get(3)?,
            title: row.get(4)?,
            added_at: row.get(5)?,
            file_hash: row.get(6)?,
        })
    })?;

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