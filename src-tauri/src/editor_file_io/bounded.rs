//! Editor-only bounded I/O. No dependency on Tauri; standalone native tests.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// Kept in sync with editorFilePolicy.ts by a frontend contract test.
pub const LARGE_FILE_LIMIT_BYTES: u64 = 10 * 1024 * 1024;
static SAVE_ID: AtomicU64 = AtomicU64::new(0);

fn regular_metadata(file: &File) -> Result<std::fs::Metadata, String> {
    let meta = file.metadata().map_err(|e| format!("metadata: {e}"))?;
    if !meta.is_file() { return Err("not a regular file".into()); }
    if meta.len() > LARGE_FILE_LIMIT_BYTES {
        return Err(format!("too_large: {} bytes; limit {}", meta.len(), LARGE_FILE_LIMIT_BYTES));
    }
    Ok(meta)
}

fn open_regular(path: &Path) -> Result<File, String> {
    // Preflight avoids special files; handle metadata below is authoritative.
    let pre = fs::symlink_metadata(path).map_err(|e| format!("metadata: {e}"))?;
    if !pre.is_file() { return Err("not a regular file (symlinks not editable)".into()); }
    let mut opts = OpenOptions::new();
    opts.read(true);
    // Do not block on a FIFO substituted between preflight and open.
    #[cfg(target_os = "linux")]
    { use std::os::unix::fs::OpenOptionsExt; opts.custom_flags(0x800 | 0x20000); } // O_NONBLOCK | O_NOFOLLOW
    let file = opts.open(path).map_err(|e| format!("open: {e}"))?;
    regular_metadata(&file)?;
    Ok(file)
}

fn read_limit_plus_one(reader: &mut impl Read, initial_size: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(initial_size.min(LARGE_FILE_LIMIT_BYTES) as usize + 1);
    reader.take(LARGE_FILE_LIMIT_BYTES + 1).read_to_end(&mut bytes)
        .map_err(|e| format!("read: {e}"))?;
    if bytes.len() as u64 > LARGE_FILE_LIMIT_BYTES { return Err("too_large: grew during read".into()); }
    Ok(bytes)
}

fn read_handle(file: &mut File) -> Result<String, String> {
    let before = regular_metadata(file)?;
    // limit+1 catches concurrent growth, without ever reading the whole body.
    let bytes = read_limit_plus_one(file, before.len())?;
    let after = regular_metadata(file)?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("file changed while reading; retry".into());
    }
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".into())
}

pub fn read_bounded(path: &Path) -> Result<String, String> {
    read_handle(&mut open_regular(path)?)
}

fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if a.dev() != b.dev() || a.ino() != b.ino() { return false; }
    }
    a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

struct StagedFile(PathBuf);
impl Drop for StagedFile { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }

/// Atomic replacement, preserving target until all bounded checks complete.
/// A caller saving an empty buffer must prove it actually loaded the old body.
pub fn write_bounded(path: &Path, content: &str, create: bool, expected: Option<&str>) -> Result<(), String> {
    if content.len() as u64 > LARGE_FILE_LIMIT_BYTES { return Err("too_large: save payload".into()); }
    if expected.is_some_and(|s| s.len() as u64 > LARGE_FILE_LIMIT_BYTES) { return Err("too_large: expected payload".into()); }
    let mut original = match open_regular(path) {
        Ok(file) => Some(file),
        Err(error) => {
            if create && fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) { None }
            else { return Err(error); }
        }
    };
    let original_meta = original.as_ref().map(regular_metadata).transpose()?;
    if let Some(file) = original.as_mut() {
        if let Some(expected) = expected {
            if read_handle(file)? != expected { return Err("conflict: file changed on disk".into()); }
        } else if content.is_empty() && original_meta.as_ref().is_some_and(|m| m.len() > 0) {
            return Err("refusing empty overwrite without loaded-content proof".into());
        }
    }
    // User requires every staging object beneath the dedicated private root.
    // Absence / cross-filesystem staging must fail, never switch to /tmp or
    // an arbitrary target parent. Environment setup defines this explicitly.
    let stage_root = std::env::var_os("ORG2_EDITOR_STAGE_DIR").ok_or("editor staging directory not configured")?;
    let parent = PathBuf::from(stage_root);
    if !parent.is_absolute() || !parent.is_dir() { return Err("invalid editor staging directory".into()); }
    let name = path.file_name().ok_or("missing filename")?.to_string_lossy();
    let tmp_path = parent.join(format!(".{name}.org2-save-{}-{}", std::process::id(), SAVE_ID.fetch_add(1, Ordering::Relaxed)));
    let mut opts = OpenOptions::new(); opts.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; opts.mode(0o600); }
    let mut staged = opts.open(&tmp_path).map_err(|e| format!("stage: {e}"))?;
    let _cleanup = StagedFile(tmp_path.clone());
    staged.write_all(content.as_bytes()).map_err(|e| format!("write: {e}"))?;
    if let Some(meta) = &original_meta { staged.set_permissions(meta.permissions()).map_err(|e| format!("permissions: {e}"))?; }
    staged.sync_all().map_err(|e| format!("sync: {e}"))?;
    if let Some(meta) = original_meta {
        let held = original.as_ref().ok_or("lost original handle")?;
        let handle_now = regular_metadata(held)?;
        let path_now = fs::symlink_metadata(path).map_err(|e| format!("revalidate: {e}"))?;
        if !path_now.is_file() || !same_file(&meta, &handle_now) || !same_file(&meta, &path_now) {
            return Err("conflict: target changed before commit".into());
        }
        fs::rename(&tmp_path, path).map_err(|e| format!("commit: {e}"))?;
    } else {
        // hard_link is atomic and fails if a concurrent creator occupied path.
        fs::hard_link(&tmp_path, path).map_err(|e| format!("create commit: {e}"))?;
    }
    // Atomic old-or-new contents are guaranteed; full power-loss directory
    // durability and a last-microsecond external replacement need a later gate.
    // A filesystem mismatch fails explicitly: no copy/truncate fallback.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture { fn new() -> Self {
        let root = std::env::var("ORG2_TEST_TMP").expect("controlled fixture path required");
        let p = PathBuf::from(root).join(format!("bounded-{}-{}", std::process::id(), SAVE_ID.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&p).unwrap(); Self(p)
    } fn path(&self) -> PathBuf { self.0.join("file.txt") } }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test] fn boundaries_and_utf8() {
        let f=Fixture::new(); let p=f.path();
        for size in [LARGE_FILE_LIMIT_BYTES-1, LARGE_FILE_LIMIT_BYTES, LARGE_FILE_LIMIT_BYTES+1] {
            let file=File::create(&p).unwrap(); file.set_len(size).unwrap();
            assert_eq!(read_bounded(&p).is_ok(), size <= LARGE_FILE_LIMIT_BYTES);
        }
        fs::write(&p, [255]).unwrap(); assert!(read_bounded(&p).unwrap_err().contains("UTF-8"));
        fs::write(&p, "中文").unwrap(); assert_eq!(read_bounded(&p).unwrap(), "中文");
    }
    #[test] fn sparse_564mb_rejects_and_never_truncates() {
        let f=Fixture::new(); let p=f.path(); File::create(&p).unwrap().set_len(564*1024*1024).unwrap();
        assert!(read_bounded(&p).unwrap_err().contains("too_large"));
        assert!(write_bounded(&p, "", false, None).is_err());
        assert_eq!(fs::metadata(&p).unwrap().len(), 564*1024*1024);
    }
    #[test] fn same_handle_survives_path_replacement_without_reading_replacement() {
        let f=Fixture::new(); let p=f.path(); fs::write(&p,"old").unwrap(); let mut held=open_regular(&p).unwrap();
        let new=f.0.join("new"); File::create(&new).unwrap().set_len(564*1024*1024).unwrap(); fs::rename(new,&p).unwrap();
        assert_eq!(read_handle(&mut held).unwrap(),"old"); assert!(read_bounded(&p).is_err());
    }
    #[test] fn concurrent_growth_body_is_capped_at_limit_plus_one() {
        struct CountingReader { count: usize }
        impl Read for CountingReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                buffer.fill(b'x'); self.count += buffer.len(); Ok(buffer.len())
            }
        }
        let mut reader = CountingReader { count: 0 };
        assert!(read_limit_plus_one(&mut reader, 3).unwrap_err().contains("too_large"));
        assert_eq!(reader.count as u64, LARGE_FILE_LIMIT_BYTES + 1);
    }
    #[test] fn save_payload_bound_never_changes_target() {
        let f=Fixture::new(); let p=f.path(); fs::write(&p,"old").unwrap();
        let huge="x".repeat(LARGE_FILE_LIMIT_BYTES as usize + 1);
        assert!(write_bounded(&p,&huge,false,Some("old")).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(),"old");
    }
    #[test] fn growth_on_held_handle_is_rejected() {
        let f=Fixture::new(); let p=f.path(); fs::write(&p,"small").unwrap(); let mut held=open_regular(&p).unwrap();
        OpenOptions::new().write(true).open(&p).unwrap().set_len(LARGE_FILE_LIMIT_BYTES+1).unwrap();
        assert!(read_handle(&mut held).is_err());
    }
    #[test] fn save_conflict_empty_safety_atomic_create_and_recovery() {
        let f=Fixture::new(); let p=f.path(); fs::write(&p,"old").unwrap();
        assert!(write_bounded(&p,"",false,None).is_err());
        assert!(write_bounded(&p,"new",false,Some("wrong")).is_err()); assert_eq!(fs::read_to_string(&p).unwrap(),"old");
        write_bounded(&p,"new",false,Some("old")).unwrap();
        write_bounded(&p,"",false,Some("new")).unwrap(); assert_eq!(fs::read_to_string(&p).unwrap(),"");
        let new=f.0.join("created"); assert!(write_bounded(&new,"new",false,None).is_err());
        write_bounded(&new,"new",true,None).unwrap();
        assert_eq!(fs::read_to_string(&new).unwrap(),"new"); assert_eq!(fs::read_dir(&f.0).unwrap().count(),2);
    }
    #[test] fn invalid_special_and_missing_files_fail_closed() {
        let f=Fixture::new(); assert!(read_bounded(&f.0).is_err()); assert!(read_bounded(&f.path()).is_err());
        #[cfg(unix)] { let p=f.path(); std::os::unix::fs::symlink(&f.0,&p).unwrap(); assert!(read_bounded(&p).is_err()); }
    }
}
