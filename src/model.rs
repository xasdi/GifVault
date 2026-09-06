#[derive(Debug, Clone)]
pub struct Gif {
    pub id: i64,
    pub source_type: String,
    pub source_path: String,
    pub local_cache_path: Option<String>,
    pub title: Option<String>,
    pub added_at: String,
    pub file_hash: Option<String>,
}