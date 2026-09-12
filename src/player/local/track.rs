use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct LocalTrack {
    pub path: PathBuf,
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub cover_path: Option<PathBuf>,
}

impl LocalTrack {
    pub fn uri_to_path(uri: &str) -> PathBuf {
        if let Some(s) = uri.strip_prefix("file://") {
            PathBuf::from(s)
        } else {
            PathBuf::from(uri)
        }
    }
}
