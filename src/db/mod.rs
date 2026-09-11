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

/// Where decoded-and-downscaled gif frames are cached, keyed by gif id, so
/// they don't need to be re-decoded from the source file on every app start.
pub fn cache_dir() -> PathBuf {
    let proj_dirs = directories::ProjectDirs::from("com", "yourcompany", "gifvault")
        .expect("could not determine app data directory");

    let cache_dir = proj_dirs.data_dir().join("cache");
    std::fs::create_dir_all(&cache_dir).expect("could not create cache directory");

    cache_dir
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

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )
    .expect("could not create tables");

    // Columns added after the initial release. `CREATE TABLE IF NOT EXISTS`
    // above doesn't touch a `gifs` table that already exists from an older
    // version, so add them here; the errors are ignored because SQLite has
    // no `ADD COLUMN IF NOT EXISTS` and these fail harmlessly once the
    // columns are already present.
    let _ = conn.execute("ALTER TABLE gifs ADD COLUMN deleted_at TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE gifs ADD COLUMN use_count INTEGER NOT NULL DEFAULT 0",
        [],
    );
    // Dimensions are known up front (peeked from the file's header at import
    // time, not a full decode) so the grid can lay a gif out correctly
    // before ever decoding its frames — see thumbnail::peek_dimensions.
    let _ = conn.execute("ALTER TABLE gifs ADD COLUMN width INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE gifs ADD COLUMN height INTEGER NOT NULL DEFAULT 0", []);

    conn
}

pub mod queries;