//! Capability-aware, fail-closed global path grants. No session context is stored.
//! The store is re-read at each operation (no revocation cache). A missing store
//! means zero grants; unreadable/malformed stores are errors, never defaults.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

static MUTATION: Mutex<()> = Mutex::new(());
const HEADER: &str = "ORG2_PATH_GRANTS_V1\n";
const MAX_STORE_BYTES: u64 = 256 * 1024;
const MAX_GRANTS: usize = 512;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub path: PathBuf,
    pub write: bool,
    pub recursive: bool,
}

/// Resolve the deepest existing ancestor, preserving symlink resolution for
/// not-yet-created descendants. Never fall back to a lexical path on I/O error.
pub fn canonical_candidate(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.to_str().is_none_or(|s| s.contains('\0')) {
        return Err("path must be absolute UTF-8 without NUL".into());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("parent traversal is not permitted".into());
    }
    let mut ancestor = path.to_path_buf();
    let mut tail = Vec::new();
    loop {
        match fs::symlink_metadata(&ancestor) {
            Ok(_) => {
                let mut canonical =
                    fs::canonicalize(&ancestor).map_err(|_| "path resolution failed")?;
                if !tail.is_empty() && !canonical.is_dir() {
                    return Err("ancestor is not a directory".into());
                }
                for name in tail.iter().rev() {
                    canonical.push(name);
                }
                return Ok(canonical);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tail.push(
                    ancestor
                        .file_name()
                        .ok_or("no existing path ancestor")?
                        .to_os_string(),
                );
                if !ancestor.pop() {
                    return Err("no existing path ancestor".into());
                }
            }
            Err(_) => return Err("path inaccessible".into()),
        }
    }
}
pub fn forbidden(candidate: &Path, denied: &[PathBuf]) -> Result<(), String> {
    let resolved = canonical_candidate(candidate)?;
    for root in denied {
        // Test both spellings, so an existing symlink in a forbidden root
        // never weakens its original lexical prohibition.
        if candidate.starts_with(root) || resolved.starts_with(&canonical_candidate(root)?) {
            return Err("forbidden path policy denied".into());
        }
    }
    Ok(())
}
pub fn matches(grants: &[Grant], candidate: &Path, access: Access) -> Result<bool, String> {
    let resolved = canonical_candidate(candidate)?;
    for grant in grants {
        if access == Access::Write && !grant.write {
            continue;
        }
        // A changed symlink/renamed root cannot broaden a persisted grant.
        let Ok(current_root) = fs::canonicalize(&grant.path) else {
            continue;
        };
        if current_root != grant.path {
            continue;
        }
        if resolved == grant.path || (grant.recursive && resolved.starts_with(&grant.path)) {
            return Ok(true);
        }
    }
    Ok(false)
}
/// Shared decision used before every filesystem tool/GUI action. Workspace
/// roots remain session-local; only explicitly stored grants cross sessions.
pub fn authorize(
    store: &Path,
    candidate: &Path,
    access: Access,
    local_roots: &[PathBuf],
    denied: &[PathBuf],
    restricted: bool,
) -> Result<(), String> {
    forbidden(candidate, denied)?;
    let grants = load(store)?; // Error is fatal even if a local root matches.
    let resolved = canonical_candidate(candidate)?;
    if !restricted {
        return Ok(());
    }
    if local_roots
        .iter()
        .any(|p| fs::canonicalize(p).is_ok_and(|root| resolved.starts_with(root)))
    {
        return Ok(());
    }
    if matches(&grants, &resolved, access)? {
        return Ok(());
    }
    Err("path authority denied: no local or explicit global capability".into())
}

/// A recursive scan cannot infer recursive authority from an exact-root grant.
pub fn authorize_tree(
    store: &Path,
    candidate: &Path,
    local_roots: &[PathBuf],
    denied: &[PathBuf],
    restricted: bool,
) -> Result<(), String> {
    authorize(
        store,
        candidate,
        Access::Read,
        local_roots,
        denied,
        restricted,
    )?;
    let root = canonical_candidate(candidate)?;
    for path in denied {
        if canonical_candidate(path)?.starts_with(&root) {
            return Err("scan scope overlaps forbidden descendant".into());
        }
    }
    if !restricted
        || local_roots
            .iter()
            .any(|p| fs::canonicalize(p).is_ok_and(|p| root.starts_with(p)))
    {
        return Ok(());
    }
    let recursive: Vec<_> = load(store)?.into_iter().filter(|g| g.recursive).collect();
    if matches(&recursive, &root, Access::Read)? {
        Ok(())
    } else {
        Err("recursive scan needs an explicit recursive grant".into())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Result<String, String> {
    if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid grant encoding".into());
    }
    let bytes: Result<Vec<_>, _> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect();
    String::from_utf8(bytes.map_err(|_| "invalid grant encoding")?)
        .map_err(|_| "invalid grant UTF-8".into())
}
pub fn load(store: &Path) -> Result<Vec<Grant>, String> {
    let meta = match fs::symlink_metadata(store) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("grant store unavailable".into()),
        Ok(m) => m,
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_STORE_BYTES {
        return Err("invalid grant store".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000);
    } // O_NOFOLLOW
    let file = options.open(store).map_err(|_| "grant store unavailable")?;
    if !file
        .metadata()
        .map_err(|_| "grant store unavailable")?
        .is_file()
    {
        return Err("invalid grant store".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "grant store unavailable")?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("invalid grant store size".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "invalid grant store encoding")?;
    let records = text
        .strip_prefix(HEADER)
        .ok_or("invalid grant store version")?;
    let mut grants = Vec::new();
    for record in records.lines() {
        let fields: Vec<_> = record.split('\t').collect();
        if fields.len() != 3 || !matches!(fields[1], "r" | "rw") || !matches!(fields[2], "0" | "1")
        {
            return Err("invalid grant record".into());
        }
        let path = PathBuf::from(unhex(fields[0])?);
        if !path.is_absolute()
            || path.parent().is_none()
            || path.components().any(|c| matches!(c, Component::ParentDir))
            || path.to_str().is_none_or(|s| s.contains('\0'))
        {
            return Err("invalid grant path".into());
        }
        let grant = Grant {
            path,
            write: fields[1] == "rw",
            recursive: fields[2] == "1",
        };
        if grants.iter().any(|g: &Grant| g.path == grant.path) {
            return Err("duplicate grant path".into());
        }
        grants.push(grant);
        if grants.len() > MAX_GRANTS {
            return Err("too many grants".into());
        }
    }
    Ok(grants)
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}
fn mutate(
    store: &Path,
    change: impl FnOnce(&mut Vec<Grant>) -> Result<(), String>,
) -> Result<Vec<Grant>, String> {
    let _guard = MUTATION.lock().map_err(|_| "grant mutation lock failed")?;
    let parent = store.parent().ok_or("invalid store parent")?;
    fs::create_dir_all(parent).map_err(|_| "cannot create grant store directory")?;
    let lock = store.with_extension("lock");
    fs::create_dir(&lock).map_err(|_| "grant store busy; stale lock requires explicit recovery")?;
    let _lock = Lock(lock);
    let mut grants = load(store)?;
    change(&mut grants)?;
    if grants.len() > MAX_GRANTS {
        return Err("too many grants".into());
    }
    grants.sort_by(|a, b| a.path.cmp(&b.path));
    let mut text = HEADER.to_string();
    for g in &grants {
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            hex(g.path.to_str().ok_or("invalid UTF-8 path")?.as_bytes()),
            if g.write { "rw" } else { "r" },
            if g.recursive { "1" } else { "0" }
        ));
    }
    if text.len() as u64 > MAX_STORE_BYTES {
        return Err("grant store capacity exceeded".into());
    }
    let stage = store.with_extension(format!("stage-{}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&stage)
        .map_err(|_| "grant staging unavailable")?;
    let result: Result<(), String> = (|| {
        file.write_all(text.as_bytes())
            .map_err(|_| "grant write failed")?;
        file.sync_all().map_err(|_| "grant fsync failed")?;
        fs::rename(&stage, store).map_err(|_| "grant commit failed")?;
        #[cfg(unix)]
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| "grant directory fsync failed")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(stage);
    }
    result?;
    Ok(grants)
}
pub fn add(
    store: &Path,
    path: &Path,
    access: Access,
    recursive: bool,
) -> Result<Vec<Grant>, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("grant path must be absolute without traversal".into());
    }
    let path = fs::canonicalize(path).map_err(|_| "grant target must exist")?;
    if path.parent().is_none() || path.to_str().is_none_or(|s| s.contains('\0')) {
        return Err("whole filesystem grants are prohibited".into());
    }
    if recursive && !path.is_dir() {
        return Err("recursive grant requires a directory".into());
    }
    mutate(store, move |grants| {
        grants.retain(|g| g.path != path);
        grants.push(Grant {
            path,
            write: access == Access::Write,
            recursive,
        });
        Ok(())
    })
}
pub fn remove(store: &Path, path: &Path) -> Result<Vec<Grant>, String> {
    // Remove by persisted identity, even when the target has since disappeared.
    if !path.is_absolute() {
        return Err("grant identity must be absolute".into());
    }
    mutate(store, |grants| {
        grants.retain(|g| g.path != path);
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        let root =
            PathBuf::from(std::env::var("ORG2_TEST_TMP").expect("private fixture root required"))
                .join(name);
        fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn crud_revocation_restart_and_access() {
        let root = fixture("crud");
        let target = root.join("target");
        fs::create_dir_all(&target).unwrap();
        let store = root.join("grants");
        remove(&store, &target).unwrap();
        assert!(load(&store).unwrap().is_empty());
        add(&store, &target, Access::Read, true).unwrap();
        let child = target.join("new/deep.txt");
        assert!(matches(&load(&store).unwrap(), &child, Access::Read).unwrap());
        assert!(!matches(&load(&store).unwrap(), &child, Access::Write).unwrap());
        add(&store, &target, Access::Write, true).unwrap();
        assert!(matches(&load(&store).unwrap(), &child, Access::Write).unwrap());
        remove(&store, &target).unwrap();
        assert!(!matches(&load(&store).unwrap(), &child, Access::Read).unwrap());
    }
    #[test]
    fn two_sessions_only_share_explicit_grants_and_forbidden_wins() {
        let root = fixture("sessions");
        let a = root.join("a");
        let b = root.join("b");
        let shared = root.join("shared");
        for dir in [&a, &b, &shared] {
            fs::create_dir_all(dir).unwrap();
        }
        let store = root.join("store");
        fs::write(&store, HEADER).unwrap();
        add(&store, &shared, Access::Read, true).unwrap();
        for local in [&a, &b] {
            assert!(authorize(
                &store,
                &shared.join("new"),
                Access::Read,
                &[local.clone()],
                &[],
                true
            )
            .is_ok());
            assert!(authorize(
                &store,
                &shared.join("new"),
                Access::Write,
                &[local.clone()],
                &[],
                true
            )
            .is_err());
        }
        assert!(authorize(&store, &b, Access::Read, &[a.clone()], &[], true).is_err());
        assert!(authorize(
            &store,
            &shared,
            Access::Read,
            &[a.clone()],
            &[shared.clone()],
            true
        )
        .is_err());
        remove(&store, &shared).unwrap();
        assert!(authorize(&store, &shared, Access::Read, &[a], &[], true).is_err());
        fs::write(&store, "broken").unwrap();
        assert!(authorize(&store, &b, Access::Read, &[b.clone()], &[], true).is_err());
    }
    #[test]
    fn malformed_large_store_bounded() {
        let root = fixture("large-store");
        let store = root.join("store");
        fs::write(&store, vec![b'x'; MAX_STORE_BYTES as usize + 1]).unwrap();
        assert!(load(&store).is_err());
    }
    #[test]
    fn scan_requires_recursive_grant_and_no_forbidden_overlap() {
        let root = fixture("scan");
        let local = root.join("local");
        let external = root.join("external");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&external).unwrap();
        let store = root.join("store");
        fs::write(&store, HEADER).unwrap();
        add(&store, &external, Access::Read, false).unwrap();
        assert!(authorize_tree(&store, &external, &[local.clone()], &[], true).is_err());
        add(&store, &external, Access::Read, true).unwrap();
        assert!(authorize_tree(&store, &external, &[local.clone()], &[], true).is_ok());
        assert!(authorize_tree(
            &store,
            &external,
            &[local],
            &[external.join("private")],
            true
        )
        .is_err());
    }
    #[test]
    fn exact_grants_and_prefix_siblings() {
        let root = fixture("exact");
        let dir = root.join("data");
        fs::create_dir_all(&dir).unwrap();
        let sibling = root.join("data-other");
        fs::create_dir_all(&sibling).unwrap();
        let grant = Grant {
            path: dir.clone(),
            write: false,
            recursive: false,
        };
        assert!(matches(&[grant.clone()], &dir, Access::Read).unwrap());
        assert!(!matches(&[grant.clone()], &dir.join("x"), Access::Read).unwrap());
        assert!(!matches(
            &[Grant {
                recursive: true,
                ..grant
            }],
            &sibling,
            Access::Read
        )
        .unwrap());
    }
    #[test]
    fn syntax_and_forbidden_priority() {
        let root = fixture("forbidden");
        assert!(canonical_candidate(Path::new("relative")).is_err());
        assert!(canonical_candidate(&root.join("../escape")).is_err());
        assert!(canonical_candidate(&root.join("a\0b")).is_err());
        assert!(forbidden(&root.join("future/deep"), &[root]).is_err());
    }
    #[test]
    fn malformed_store_fails_closed() {
        let root = fixture("malformed");
        let store = root.join("store");
        fs::write(&store, "broken").unwrap();
        assert!(load(&store).is_err());
        assert!(add(&store, &root, Access::Read, true).is_err());
        fs::write(&store, format!("{HEADER}zz\tr\t1\n")).unwrap();
        assert!(load(&store).is_err());
    }
    #[test]
    fn no_whole_filesystem_or_invalid_recursive_grants() {
        let root = fixture("invalid");
        let store = root.join("store");
        assert!(add(&store, Path::new("/"), Access::Write, true).is_err());
        let file = root.join("file");
        fs::write(&file, "").unwrap();
        assert!(add(&store, &file, Access::Read, true).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_escape_existing_and_deep_future() {
        use std::os::unix::fs::symlink;
        let root = fixture("escape");
        let dir = root.join("allowed");
        let outside = root.join("outside");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let link = dir.join("link");
        if !link.exists() {
            symlink(&outside, &link).unwrap();
        }
        let grants = [Grant {
            path: dir,
            write: true,
            recursive: true,
        }];
        assert!(!matches(&grants, &link.join("new/deep.txt"), Access::Write).unwrap());
        assert!(forbidden(&link.join("deep"), &[outside]).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_store_denied() {
        use std::os::unix::fs::symlink;
        let root = fixture("storelink");
        let target = root.join("target");
        fs::write(&target, HEADER).unwrap();
        let link = root.join("link");
        if fs::symlink_metadata(&link).is_err() {
            symlink(&target, &link).unwrap();
        }
        assert!(load(&link).is_err());
    }
    #[test]
    fn cross_process_busy_is_not_overwritten() {
        let root = fixture("busy");
        let store = root.join("store");
        let lock = store.with_extension("lock");
        fs::create_dir(&lock).unwrap();
        assert!(add(&store, &root, Access::Read, true).is_err());
        fs::remove_dir(lock).unwrap();
    }
}
