//! Publish pinned source downloads only after their checksum passes.
use super::Result;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn reserve(path: PathBuf) -> Result<Pending> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(Pending(path))
}

fn matches(path: &Path, sha: &str) -> Result<bool> {
    match fs::read(path) {
        Ok(bytes) => Ok(format!("{:x}", Sha256::digest(bytes)) == sha),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn fetch(
    cache: &Path,
    name: &str,
    sha: &str,
    download: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    let path = cache.join(name);
    if matches(&path, sha)? {
        return Ok(path);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let staging = cache.join(format!(
        ".{name}.{}-{}.partial",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // Reserve an owned file so cleanup cannot remove another caller's output.
    let pending = reserve(staging)?;
    download(&pending.0)?;
    if !matches(&pending.0, sha)? {
        return Err(format!("Pinned CPU source hash mismatch: {}", path.display()).into());
    }
    fs::rename(&pending.0, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Cache(PathBuf);
    impl Cache {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "daisy-source-cache-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn only_archive_remains(&self) {
            let names: Vec<_> = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(names, [std::ffi::OsString::from("source.tar.gz")]);
        }
    }
    impl Drop for Cache {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    #[test]
    fn staging_collision_does_not_remove_another_callers_file() {
        let cache = Cache::new();
        let path = cache.0.join("occupied.partial");
        fs::write(&path, b"another caller").unwrap();
        assert!(reserve(path.clone()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"another caller");
    }
    #[test]
    fn verified_cache_does_not_need_a_network_connection() {
        let cache = Cache::new();
        fs::write(cache.0.join("source.tar.gz"), b"complete").unwrap();
        fetch(&cache.0, "source.tar.gz", &hash(b"complete"), |_| {
            panic!("cached download was repeated")
        })
        .unwrap();
        cache.only_archive_remains();
    }
    #[test]
    fn failed_partial_download_keeps_existing_cache_and_cleans_staging() {
        let cache = Cache::new();
        fs::write(cache.0.join("source.tar.gz"), b"old partial").unwrap();
        let result = fetch(&cache.0, "source.tar.gz", &hash(b"complete"), |path| {
            fs::write(path, b"new partial")?;
            Err("connection interrupted".into())
        });
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("connection interrupted"));
        assert_eq!(
            fs::read(cache.0.join("source.tar.gz")).unwrap(),
            b"old partial"
        );
        cache.only_archive_remains();
    }
    #[test]
    fn checksum_mismatch_never_publishes_downloaded_bytes() {
        let cache = Cache::new();
        assert!(
            fetch(&cache.0, "source.tar.gz", &hash(b"complete"), |path| {
                fs::write(path, b"corrupt")?;
                Ok(())
            })
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
        );
        assert_eq!(fs::read_dir(&cache.0).unwrap().count(), 0);
    }
    #[test]
    fn verified_download_repairs_an_interrupted_older_cache() {
        let cache = Cache::new();
        fs::write(cache.0.join("source.tar.gz"), b"old partial").unwrap();
        let path = fetch(&cache.0, "source.tar.gz", &hash(b"complete"), |path| {
            fs::write(path, b"complete")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), b"complete");
        cache.only_archive_remains();
    }
}
