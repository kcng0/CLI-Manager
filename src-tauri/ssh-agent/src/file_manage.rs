use crate::files::{relative_path, resolve_relative, resolve_root, RemoteFileEntry};
use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use std::fs;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAX_MANAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 512 * 1024;
const MAX_TEXT_WRITE_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCreateRequest {
    pub root_path: String,
    #[serde(default)]
    pub parent_path: String,
    pub name: String,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRenameRequest {
    pub root_path: String,
    pub relative_path: String,
    pub new_name: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDeleteRequest {
    pub root_path: String,
    pub relative_path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferRequest {
    pub root_path: String,
    pub source_path: String,
    pub target_parent_path: String,
    pub name: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWriteTextRequest {
    pub root_path: String,
    pub relative_path: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWriteBytesRequest {
    pub root_path: String,
    #[serde(default)]
    pub parent_path: String,
    pub name: String,
    pub data_base64: String,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub offset: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatRequest {
    pub root_path: String,
    pub relative_path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadBytesRequest {
    pub root_path: String,
    pub relative_path: String,
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub length: u64,
}

pub fn create(request: FileCreateRequest) -> Result<RemoteFileEntry, String> {
    let root = resolve_root(&request.root_path)?;
    let target = resolve_new_child(&root, &request.parent_path, &request.name)?;
    prepare_target(&target, request.overwrite)?;
    if request.kind == "directory" {
        fs::create_dir(&target).map_err(|_| "remote_file_create_failed".to_string())?;
    } else {
        fs::write(&target, []).map_err(|_| "remote_file_create_failed".to_string())?;
    }
    stat_path(&root, &target)
}

pub fn rename(request: FileRenameRequest) -> Result<RemoteFileEntry, String> {
    let root = resolve_root(&request.root_path)?;
    if request.relative_path.is_empty() {
        return Err("cannot_delete_root".to_string());
    }
    let source = resolve_existing_plain(&root, &request.relative_path)?;
    let parent_rel = parent_relative(&request.relative_path);
    let target = resolve_new_child(&root, &parent_rel, &request.new_name)?;
    if source == target {
        return stat_path(&root, &source);
    }
    prepare_target(&target, request.overwrite)?;
    fs::rename(&source, &target).map_err(|_| "remote_file_rename_failed".to_string())?;
    stat_path(&root, &target)
}

pub fn delete(request: FileDeleteRequest) -> Result<(), String> {
    let root = resolve_root(&request.root_path)?;
    if request.relative_path.is_empty() {
        return Err("cannot_delete_root".to_string());
    }
    let path = resolve_existing_plain(&root, &request.relative_path)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| "remote_file_not_found".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("remote_file_path_confined".to_string());
    }
    if metadata.is_dir() {
        fs::remove_dir_all(&path).map_err(|_| "remote_file_delete_failed".to_string())
    } else {
        fs::remove_file(&path).map_err(|_| "remote_file_delete_failed".to_string())
    }
}

pub fn copy(request: FileTransferRequest) -> Result<RemoteFileEntry, String> {
    transfer(request, false)
}

pub fn move_entry(request: FileTransferRequest) -> Result<RemoteFileEntry, String> {
    transfer(request, true)
}

pub fn write_text(request: FileWriteTextRequest) -> Result<RemoteFileEntry, String> {
    if request.content.len() > MAX_TEXT_WRITE_BYTES {
        return Err("remote_file_too_large".to_string());
    }
    let root = resolve_root(&request.root_path)?;
    let (parent_rel, name) = split_relative(&request.relative_path)?;
    let target = resolve_new_child(&root, &parent_rel, &name)?;
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        if metadata.file_type().is_symlink() || metadata.is_dir() {
            return Err("remote_file_path_confined".to_string());
        }
    }
    fs::write(&target, request.content.as_bytes())
        .map_err(|_| "remote_file_write_failed".to_string())?;
    stat_path(&root, &target)
}

pub fn write_bytes(request: FileWriteBytesRequest) -> Result<RemoteFileEntry, String> {
    let data = general_purpose::STANDARD
        .decode(request.data_base64.as_bytes())
        .map_err(|_| "remote_file_bytes_invalid".to_string())?;
    if data.len() > MAX_CHUNK_BYTES {
        return Err("remote_file_chunk_too_large".to_string());
    }
    if data.is_empty() && request.offset != 0 {
        return Err("attachment_empty".to_string());
    }
    let end = request
        .offset
        .checked_add(data.len() as u64)
        .ok_or_else(|| "remote_file_too_large".to_string())?;
    if end > MAX_MANAGE_BYTES {
        return Err("remote_file_too_large".to_string());
    }
    let root = resolve_root(&request.root_path)?;
    let target = resolve_new_child(&root, &request.parent_path, &request.name)?;
    if request.offset == 0 {
        prepare_target(&target, request.overwrite)?;
        fs::write(&target, &data).map_err(|_| "remote_file_write_failed".to_string())?;
        return stat_path(&root, &target);
    }
    let metadata =
        fs::symlink_metadata(&target).map_err(|_| "remote_file_not_found".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("remote_file_path_confined".to_string());
    }
    if request.offset > metadata.len() {
        return Err("remote_file_offset_invalid".to_string());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .open(&target)
        .map_err(|_| "remote_file_write_failed".to_string())?;
    file.seek(SeekFrom::Start(request.offset))
        .map_err(|_| "remote_file_write_failed".to_string())?;
    file.write_all(&data)
        .map_err(|_| "remote_file_write_failed".to_string())?;
    stat_path(&root, &target)
}

pub fn read_bytes(request: FileReadBytesRequest) -> Result<serde_json::Value, String> {
    let root = resolve_root(&request.root_path)?;
    let path = resolve_existing_plain(&root, &request.relative_path)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| "remote_file_not_found".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("remote_file_not_file".to_string());
    }
    if metadata.len() > MAX_MANAGE_BYTES {
        return Err("remote_file_too_large".to_string());
    }
    if request.offset > metadata.len() {
        return Err("remote_file_offset_invalid".to_string());
    }
    let length = if request.length == 0 {
        MAX_CHUNK_BYTES as u64
    } else {
        request.length
    };
    if length > MAX_CHUNK_BYTES as u64 {
        return Err("remote_file_chunk_too_large".to_string());
    }
    let remaining = metadata.len() - request.offset;
    let to_read = remaining.min(length) as usize;
    let mut data = vec![0u8; to_read];
    if to_read > 0 {
        let mut file = fs::File::open(&path).map_err(|_| "remote_file_read_failed".to_string())?;
        file.seek(SeekFrom::Start(request.offset))
            .map_err(|_| "remote_file_read_failed".to_string())?;
        file.read_exact(&mut data)
            .map_err(|_| "remote_file_read_failed".to_string())?;
    }
    Ok(serde_json::json!({
        "name": path.file_name().and_then(|value| value.to_str()).unwrap_or_default(),
        "relativePath": relative_path(&root, &path)?,
        "sizeBytes": metadata.len(),
        "offset": request.offset,
        "dataBase64": general_purpose::STANDARD.encode(data),
        "eof": request.offset + to_read as u64 >= metadata.len(),
    }))
}

pub fn stat(request: FileStatRequest) -> Result<RemoteFileEntry, String> {
    let root = resolve_root(&request.root_path)?;
    let path = if request.relative_path.is_empty() {
        root.clone()
    } else {
        resolve_existing_plain(&root, &request.relative_path)?
    };
    stat_path(&root, &path)
}

fn transfer(request: FileTransferRequest, moving: bool) -> Result<RemoteFileEntry, String> {
    let root = resolve_root(&request.root_path)?;
    if request.source_path.is_empty() {
        return Err("cannot_delete_root".to_string());
    }
    let source = resolve_existing_plain(&root, &request.source_path)?;
    let target = resolve_new_child(&root, &request.target_parent_path, &request.name)?;
    if target.starts_with(&source) {
        return Err("target_inside_source".to_string());
    }
    if source == target {
        return stat_path(&root, &source);
    }
    prepare_target(&target, request.overwrite)?;
    if moving {
        match fs::rename(&source, &target) {
            Ok(()) => {}
            Err(_) => {
                copy_plain(&source, &target)?;
                delete(FileDeleteRequest {
                    root_path: request.root_path,
                    relative_path: request.source_path,
                })?;
            }
        }
    } else {
        copy_plain(&source, &target)?;
    }
    stat_path(&root, &target)
}

fn copy_plain(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|_| "remote_file_not_found".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("remote_file_path_confined".to_string());
    }
    if metadata.is_dir() {
        copy_dir(source, target)
    } else {
        fs::copy(source, target)
            .map(|_| ())
            .map_err(|_| "remote_file_copy_failed".to_string())
    }
}

fn copy_dir(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir(target).map_err(|_| "remote_file_copy_failed".to_string())?;
    for entry in fs::read_dir(source).map_err(|_| "remote_file_copy_failed".to_string())? {
        let entry = entry.map_err(|_| "remote_file_copy_failed".to_string())?;
        let file_type = entry
            .file_type()
            .map_err(|_| "remote_file_copy_failed".to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let child_target = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &child_target)?;
        } else {
            fs::copy(entry.path(), child_target)
                .map_err(|_| "remote_file_copy_failed".to_string())?;
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("empty_name".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("invalid_name".to_string());
    }
    if trimmed.contains(['/', '\\', '\0', '\r', '\n']) {
        return Err("name_contains_separator".to_string());
    }
    Ok(trimmed.to_string())
}

fn resolve_parent(root: &Path, parent_rel: &str) -> Result<PathBuf, String> {
    let parent = resolve_relative(root, parent_rel)?;
    let metadata =
        fs::symlink_metadata(&parent).map_err(|_| "remote_file_not_found".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("remote_file_not_directory".to_string());
    }
    Ok(parent)
}

fn resolve_new_child(root: &Path, parent_rel: &str, name: &str) -> Result<PathBuf, String> {
    let parent = resolve_parent(root, parent_rel)?;
    let name = validate_name(name)?;
    let target = parent.join(name);
    if !target.starts_with(root) {
        return Err("remote_file_path_confined".to_string());
    }
    Ok(target)
}

fn resolve_existing_plain(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = resolve_relative(root, relative)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| "remote_file_not_found".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("remote_file_path_confined".to_string());
    }
    Ok(path)
}

fn prepare_target(path: &Path, overwrite: bool) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !overwrite {
                return Err("target_exists".to_string());
            }
            if metadata.file_type().is_symlink() {
                return Err("remote_file_path_confined".to_string());
            }
            if metadata.is_dir() {
                fs::remove_dir_all(path).map_err(|_| "remote_file_delete_failed".to_string())?;
            } else {
                fs::remove_file(path).map_err(|_| "remote_file_delete_failed".to_string())?;
            }
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err("remote_file_metadata_failed".to_string()),
    }
}

fn parent_relative(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn split_relative(path: &str) -> Result<(String, String), String> {
    let normalized = path.trim().trim_start_matches('/');
    if normalized.is_empty() {
        return Err("remote_file_path_invalid".to_string());
    }
    match normalized.rsplit_once('/') {
        Some((parent, name)) => Ok((parent.to_string(), name.to_string())),
        None => Ok((String::new(), normalized.to_string())),
    }
}

fn stat_path(root: &Path, path: &Path) -> Result<RemoteFileEntry, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "remote_file_not_found".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("remote_file_path_confined".to_string());
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(RemoteFileEntry {
        name,
        relative_path: if path == root {
            String::new()
        } else {
            relative_path(root, path)?
        },
        kind: if metadata.is_dir() {
            "directory".to_string()
        } else {
            "file".to_string()
        },
        size_bytes: metadata.len(),
        modified_ms: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        copy, create, delete, move_entry, read_bytes, rename, write_bytes, write_text,
        FileCreateRequest, FileDeleteRequest, FileReadBytesRequest, FileRenameRequest,
        FileTransferRequest, FileWriteBytesRequest, FileWriteTextRequest, MAX_CHUNK_BYTES,
    };
    use base64::{engine::general_purpose, Engine as _};
    use std::fs;

    fn root_path(root: &tempfile::TempDir) -> String {
        root.path().canonicalize().unwrap().display().to_string()
    }

    #[test]
    fn create_rename_copy_move_and_delete_stay_inside_root() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root_path(&root);
        create(FileCreateRequest {
            root_path: root_path.clone(),
            parent_path: String::new(),
            name: "notes.txt".into(),
            overwrite: false,
            kind: "file".into(),
        })
        .unwrap();
        write_text(FileWriteTextRequest {
            root_path: root_path.clone(),
            relative_path: "notes.txt".into(),
            content: "hello".into(),
        })
        .unwrap();
        rename(FileRenameRequest {
            root_path: root_path.clone(),
            relative_path: "notes.txt".into(),
            new_name: "hello.txt".into(),
            overwrite: false,
        })
        .unwrap();
        create(FileCreateRequest {
            root_path: root_path.clone(),
            parent_path: String::new(),
            name: "docs".into(),
            overwrite: false,
            kind: "directory".into(),
        })
        .unwrap();
        copy(FileTransferRequest {
            root_path: root_path.clone(),
            source_path: "hello.txt".into(),
            target_parent_path: "docs".into(),
            name: "hello.txt".into(),
            overwrite: false,
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("docs/hello.txt")).unwrap(),
            "hello"
        );
        move_entry(FileTransferRequest {
            root_path: root_path.clone(),
            source_path: "hello.txt".into(),
            target_parent_path: "docs".into(),
            name: "moved.txt".into(),
            overwrite: false,
        })
        .unwrap();
        assert!(!root.path().join("hello.txt").exists());
        delete(FileDeleteRequest {
            root_path: root_path.clone(),
            relative_path: "docs/moved.txt".into(),
        })
        .unwrap();
        assert!(!root.path().join("docs/moved.txt").exists());
        assert_eq!(
            delete(FileDeleteRequest {
                root_path,
                relative_path: String::new(),
            })
            .unwrap_err(),
            "cannot_delete_root"
        );
    }

    #[test]
    fn bytes_round_trip_and_overwrite_guard() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root_path(&root);
        let encoded = general_purpose::STANDARD.encode(b"payload");
        write_bytes(FileWriteBytesRequest {
            root_path: root_path.clone(),
            parent_path: String::new(),
            name: "data.bin".into(),
            data_base64: encoded,
            overwrite: false,
            offset: 0,
        })
        .unwrap();
        assert_eq!(
            write_bytes(FileWriteBytesRequest {
                root_path: root_path.clone(),
                parent_path: String::new(),
                name: "data.bin".into(),
                data_base64: general_purpose::STANDARD.encode(b"other"),
                overwrite: false,
                offset: 0,
            })
            .unwrap_err(),
            "target_exists"
        );
        let payload = read_bytes(FileReadBytesRequest {
            root_path,
            relative_path: "data.bin".into(),
            offset: 0,
            length: 0,
        })
        .unwrap();
        assert_eq!(payload["sizeBytes"], 7);
        assert_eq!(payload["eof"], true);
        assert_eq!(payload["dataBase64"], general_purpose::STANDARD.encode(b"payload"));
    }

    #[test]
    fn bytes_round_trip_uses_512kib_chunks() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root_path(&root);
        let first = vec![0xABu8; MAX_CHUNK_BYTES];
        let second = vec![0xCDu8; 128 * 1024];
        write_bytes(FileWriteBytesRequest {
            root_path: root_path.clone(),
            parent_path: String::new(),
            name: "chunk.bin".into(),
            data_base64: general_purpose::STANDARD.encode(&first),
            overwrite: false,
            offset: 0,
        })
        .unwrap();
        write_bytes(FileWriteBytesRequest {
            root_path: root_path.clone(),
            parent_path: String::new(),
            name: "chunk.bin".into(),
            data_base64: general_purpose::STANDARD.encode(&second),
            overwrite: false,
            offset: first.len() as u64,
        })
        .unwrap();
        assert_eq!(
            write_bytes(FileWriteBytesRequest {
                root_path: root_path.clone(),
                parent_path: String::new(),
                name: "chunk.bin".into(),
                data_base64: general_purpose::STANDARD.encode(vec![0u8; MAX_CHUNK_BYTES + 1]),
                overwrite: true,
                offset: 0,
            })
            .unwrap_err(),
            "remote_file_chunk_too_large"
        );
        let head = read_bytes(FileReadBytesRequest {
            root_path: root_path.clone(),
            relative_path: "chunk.bin".into(),
            offset: 0,
            length: MAX_CHUNK_BYTES as u64,
        })
        .unwrap();
        assert_eq!(head["sizeBytes"], (first.len() + second.len()) as u64);
        assert_eq!(head["eof"], false);
        assert_eq!(
            general_purpose::STANDARD
                .decode(head["dataBase64"].as_str().unwrap())
                .unwrap(),
            first
        );
        let tail = read_bytes(FileReadBytesRequest {
            root_path,
            relative_path: "chunk.bin".into(),
            offset: first.len() as u64,
            length: MAX_CHUNK_BYTES as u64,
        })
        .unwrap();
        assert_eq!(tail["eof"], true);
        assert_eq!(
            general_purpose::STANDARD
                .decode(tail["dataBase64"].as_str().unwrap())
                .unwrap(),
            second
        );
    }

    #[test]
    fn copy_rejects_directory_into_itself() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root_path(&root);
        create(FileCreateRequest {
            root_path: root_path.clone(),
            parent_path: String::new(),
            name: "docs".into(),
            overwrite: false,
            kind: "directory".into(),
        })
        .unwrap();
        create(FileCreateRequest {
            root_path: root_path.clone(),
            parent_path: "docs".into(),
            name: "nested".into(),
            overwrite: false,
            kind: "directory".into(),
        })
        .unwrap();
        assert_eq!(
            copy(FileTransferRequest {
                root_path,
                source_path: "docs".into(),
                target_parent_path: "docs/nested".into(),
                name: "docs".into(),
                overwrite: true,
            })
            .unwrap_err(),
            "target_inside_source"
        );
    }
}
