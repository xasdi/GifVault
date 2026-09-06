use rusqlite::Connection;
use std::path::PathBuf;

pub fn db_path() -> PathBuf {
    let proj_dirs = directories::ProjectDirs::from("com", "yourcompany", "gifvault")
        .expect("could not determine app data directory");

    let data_dir = proj_dirs.data_dir();
    std::fs::create_dir_all(data_dir).expect("could not create data directory");

    data_dir.join("gifs.db")
}

pub fn gifs_storage_dir() -> PathBuf {
    let proj_dirs = directories::ProjectDirs::from("com", "yourcompany", "gifvault")
        .expect("could not determine app data directory");

    let storage_dir = proj_dirs.data_dir().join("gifs");
    std::fs::create_dir_all(&storage_dir).expect("could not create gifs storage directory");

    storage_dir
}

pub fn init_db() -> Connection {
    let path = db_path();
    let conn = Connection::open(&path).expect("could not open database");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS gifs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_type TEXT NOT NULL,
            source_path TEXT NOT NULL,
            local_cache_path TEXT,
            title TEXT,
            added_at TEXT NOT NULL,
            file_hash TEXT
        );

        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS gif_tags (
            gif_id INTEGER NOT NULL REFERENCES gifs(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (gif_id, tag_id)
        );
        ",
    )
    .expect("could not create tables");

    conn
}

pub mod queries;