use crate::daemon::client::{DaemonBridge, DaemonClient};
use crate::ssh_launch::SshLaunchPlan;
use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

const LEGACY_IMAGE_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_LEGACY_IMAGE_PIXELS: u64 = 12_000_000;
const MAX_ATTACHMENT_BASE64_BYTES: usize = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4;
const ATTACHMENT_CHUNK_BYTES: usize = 512 * 1024;
const LEGACY_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

#[derive(Clone, Copy)]
enum AttachmentProtocol {
    LegacyImage,
    AnyFile,
}

impl AttachmentProtocol {
    fn kind(self, operation: &str) -> String {
        match self {
            Self::LegacyImage => format!("fileAttach{operation}"),
            Self::AnyFile => format!("fileAttachAny{operation}"),
        }
    }
}

enum AttachmentSource {
    Data {
        file_name: String,
        data_base64: String,
    },
    LocalPath(String),
}

impl AttachmentSource {
    fn read(self) -> Result<(String, Vec<u8>), String> {
        match self {
            Self::Data {
                file_name,
                data_base64,
            } => decode_attachment(file_name, data_base64),
            Self::LocalPath(path) => read_attachment(path),
        }
    }
}

fn validate_plan(plan: &SshLaunchPlan) -> Result<(), String> {
    if plan.host_id.trim().is_empty()
        || plan.agent_path.trim().is_empty()
        || plan.agent_installation_id.trim().is_empty()
        || plan.agent_remote_machine_id.trim().is_empty()
        || plan.client_instance_id.trim().is_empty()
    {
        return Err("remote_file_plan_invalid".to_string());
    }
    Ok(())
}

async fn request(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    kind: &'static str,
    payload: Value,
) -> Result<Value, String> {
    validate_plan(&ssh_launch)?;
    let client = daemon_bridge
        .get()
        .ok_or_else(|| "daemon_unavailable".to_string())?;
    tokio::task::spawn_blocking(move || {
        client.ssh_agent_request(consumer_id, ssh_launch, kind.to_string(), payload)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn read_managed_file_bytes(
    client: &DaemonClient,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
) -> Result<Value, String> {
    let mut offset = 0u64;
    let mut collected = Vec::new();
    let mut name = String::new();
    let mut total_size = 0u64;
    loop {
        let response = client.ssh_agent_request(
            consumer_id.clone(),
            ssh_launch.clone(),
            "fileReadBytes".to_string(),
            json!({
                "rootPath": root_path,
                "relativePath": relative_path,
                "offset": offset,
                "length": ATTACHMENT_CHUNK_BYTES,
            }),
        )?;
        if name.is_empty() {
            name = response
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
        total_size = response
            .get("sizeBytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| "remote_file_read_failed".to_string())?;
        if total_size > MAX_ATTACHMENT_BYTES as u64 {
            return Err("remote_file_too_large".to_string());
        }
        let chunk = general_purpose::STANDARD
            .decode(
                response
                    .get("dataBase64")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "remote_file_bytes_invalid".to_string())?,
            )
            .map_err(|_| "remote_file_bytes_invalid".to_string())?;
        if collected.len().saturating_add(chunk.len()) as u64 > MAX_ATTACHMENT_BYTES as u64 {
            return Err("remote_file_too_large".to_string());
        }
        collected.extend_from_slice(&chunk);
        let eof = response.get("eof").and_then(Value::as_bool).unwrap_or(false);
        offset = collected.len() as u64;
        if eof || offset >= total_size {
            break;
        }
        if chunk.is_empty() {
            return Err("remote_file_read_failed".to_string());
        }
    }
    Ok(json!({
        "name": name,
        "relativePath": relative_path,
        "sizeBytes": collected.len() as u64,
        "dataBase64": general_purpose::STANDARD.encode(collected),
    }))
}

fn write_managed_file_bytes(
    client: &DaemonClient,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    parent_path: String,
    name: String,
    data: Vec<u8>,
    overwrite: bool,
) -> Result<Value, String> {
    if data.len() > MAX_ATTACHMENT_BYTES {
        return Err("remote_file_too_large".to_string());
    }
    let mut last = Value::Null;
    let mut offset = 0u64;
    if data.is_empty() {
        return client.ssh_agent_request(
            consumer_id,
            ssh_launch,
            "fileWriteBytes".to_string(),
            json!({
                "rootPath": root_path,
                "parentPath": parent_path,
                "name": name,
                "dataBase64": "",
                "overwrite": overwrite,
                "offset": 0,
            }),
        );
    }
    for chunk in data.chunks(ATTACHMENT_CHUNK_BYTES) {
        last = client.ssh_agent_request(
            consumer_id.clone(),
            ssh_launch.clone(),
            "fileWriteBytes".to_string(),
            json!({
                "rootPath": root_path,
                "parentPath": parent_path,
                "name": name,
                "dataBase64": general_purpose::STANDARD.encode(chunk),
                "overwrite": overwrite,
                "offset": offset,
            }),
        )?;
        offset += chunk.len() as u64;
    }
    Ok(last)
}

fn validate_attachment_name(file_name: &str) -> Result<(), String> {
    if file_name.is_empty()
        || file_name.len() > 255
        || matches!(file_name, "." | "..")
        || file_name.contains(['\0', '\r', '\n', '/', '\\'])
    {
        return Err("attachment_name_invalid".to_string());
    }
    Ok(())
}

fn is_legacy_image_name(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| LEGACY_IMAGE_EXTENSIONS.contains(&extension.as_str()))
}

fn can_fallback_to_legacy_image(file_name: &str, data: &[u8]) -> bool {
    if data.len() > LEGACY_IMAGE_ATTACHMENT_BYTES || !is_legacy_image_name(file_name) {
        return false;
    }
    let Ok(reader) = image::ImageReader::new(Cursor::new(data)).with_guessed_format() else {
        return false;
    };
    reader
        .into_dimensions()
        .ok()
        .is_some_and(|(width, height)| {
            (width as u64).saturating_mul(height as u64) <= MAX_LEGACY_IMAGE_PIXELS
        })
}

fn decode_attachment(file_name: String, data_base64: String) -> Result<(String, Vec<u8>), String> {
    validate_attachment_name(&file_name)?;
    if data_base64.is_empty() || data_base64.len() > MAX_ATTACHMENT_BASE64_BYTES {
        return Err("attachment_data_invalid".to_string());
    }
    let data = general_purpose::STANDARD
        .decode(data_base64)
        .map_err(|_| "attachment_data_invalid".to_string())?;
    validate_attachment_bytes(&data)?;
    Ok((file_name, data))
}

fn read_attachment(path: String) -> Result<(String, Vec<u8>), String> {
    if path.is_empty() || path.contains(['\0', '\r', '\n']) || !Path::new(&path).is_absolute() {
        return Err("attachment_local_path_invalid".to_string());
    }
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| "attachment_local_file_unavailable".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("attachment_local_path_invalid".to_string());
    }
    if metadata.len() == 0 || metadata.len() > MAX_ATTACHMENT_BYTES as u64 {
        return Err(if metadata.len() == 0 {
            "attachment_empty".to_string()
        } else {
            "attachment_too_large".to_string()
        });
    }
    let canonical = Path::new(&path)
        .canonicalize()
        .map_err(|_| "attachment_local_file_unavailable".to_string())?;
    let file_name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "attachment_name_invalid".to_string())?
        .to_string();
    validate_attachment_name(&file_name)?;
    let file =
        fs::File::open(canonical).map_err(|_| "attachment_local_file_unavailable".to_string())?;
    let mut data = Vec::with_capacity((metadata.len() as usize).min(MAX_ATTACHMENT_BYTES));
    file.take(MAX_ATTACHMENT_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| "attachment_local_file_unavailable".to_string())?;
    validate_attachment_bytes(&data)?;
    Ok((file_name, data))
}

fn validate_attachment_bytes(data: &[u8]) -> Result<(), String> {
    if data.is_empty() {
        return Err("attachment_empty".to_string());
    }
    if data.len() > MAX_ATTACHMENT_BYTES {
        return Err("attachment_too_large".to_string());
    }
    Ok(())
}

fn validate_remote_attachment_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > 4096
        || !path.starts_with('/')
        || path.contains(['\0', '\r', '\n', '\\'])
        || path.split('/').any(|part| part == "..")
        || !path.contains("/cli-manager-ssh-agent/attachments/")
    {
        return Err("attachment_remote_path_invalid".to_string());
    }
    Ok(())
}

fn upload_attachment(
    client: &crate::daemon::client::DaemonClient,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    session_id: String,
    file_name: String,
    data: Vec<u8>,
) -> Result<String, String> {
    validate_plan(&ssh_launch)?;
    validate_attachment_name(&file_name)?;
    validate_attachment_bytes(&data)?;
    let sha256 = format!("{:x}", Sha256::digest(&data));
    let begin_payload = json!({
        "sessionId": session_id,
        "fileName": file_name,
        "sizeBytes": data.len(),
        "sha256": sha256,
    });
    let mut protocol = AttachmentProtocol::AnyFile;
    let begin = match client.ssh_agent_request(
        consumer_id.clone(),
        ssh_launch.clone(),
        protocol.kind("Begin"),
        begin_payload.clone(),
    ) {
        Ok(response) => response,
        Err(error)
            if error == "ssh_agent_capability_missing:fileAttachAny"
                && can_fallback_to_legacy_image(&file_name, &data) =>
        {
            protocol = AttachmentProtocol::LegacyImage;
            client.ssh_agent_request(
                consumer_id.clone(),
                ssh_launch.clone(),
                protocol.kind("Begin"),
                begin_payload,
            )?
        }
        Err(error) => return Err(error),
    };
    let upload_id = begin
        .get("uploadId")
        .and_then(Value::as_str)
        .filter(|value| uuid::Uuid::parse_str(value).is_ok())
        .ok_or_else(|| "attachment_begin_response_invalid".to_string())?
        .to_string();

    let result = (|| {
        let mut offset = 0usize;
        for chunk in data.chunks(ATTACHMENT_CHUNK_BYTES) {
            let expected = offset + chunk.len();
            let response = client.ssh_agent_request(
                consumer_id.clone(),
                ssh_launch.clone(),
                protocol.kind("Chunk"),
                json!({
                    "uploadId": upload_id,
                    "offset": offset,
                    "dataBase64": general_purpose::STANDARD.encode(chunk),
                }),
            )?;
            if response.get("receivedBytes").and_then(Value::as_u64) != Some(expected as u64) {
                return Err("attachment_chunk_response_invalid".to_string());
            }
            offset = expected;
        }
        let response = client.ssh_agent_request(
            consumer_id.clone(),
            ssh_launch.clone(),
            protocol.kind("Finish"),
            json!({ "uploadId": upload_id }),
        )?;
        if response.get("sizeBytes").and_then(Value::as_u64) != Some(data.len() as u64) {
            return Err("attachment_finish_response_invalid".to_string());
        }
        let path = response
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "attachment_finish_response_invalid".to_string())?;
        validate_remote_attachment_path(path)?;
        Ok(path.to_string())
    })();

    if result.is_err() {
        let _ = client.ssh_agent_request(
            consumer_id,
            ssh_launch,
            protocol.kind("Abort"),
            json!({ "uploadId": upload_id }),
        );
    }
    result
}

async fn attach(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    session_id: String,
    attachment: AttachmentSource,
) -> Result<String, String> {
    let client = daemon_bridge
        .get()
        .ok_or_else(|| "daemon_unavailable".to_string())?;
    tokio::task::spawn_blocking(move || {
        let (file_name, data) = attachment.read()?;
        upload_attachment(
            client.as_ref(),
            consumer_id,
            ssh_launch,
            session_id,
            file_name,
            data,
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn ssh_remote_file_attach_data(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    session_id: String,
    file_name: String,
    data_base64: String,
) -> Result<String, String> {
    attach(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        session_id,
        AttachmentSource::Data {
            file_name,
            data_base64,
        },
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_attach_path(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    session_id: String,
    local_path: String,
) -> Result<String, String> {
    attach(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        session_id,
        AttachmentSource::LocalPath(local_path),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_list(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileList",
        json!({ "rootPath": root_path, "relativePath": relative_path }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_read(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileRead",
        json!({ "rootPath": root_path, "relativePath": relative_path }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_search(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    query: String,
    content: bool,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileSearch",
        json!({ "rootPath": root_path, "query": query, "content": content }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_create(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    parent_path: String,
    name: String,
    kind: String,
    overwrite: bool,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileCreate",
        json!({
            "rootPath": root_path,
            "parentPath": parent_path,
            "name": name,
            "kind": kind,
            "overwrite": overwrite
        }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_rename(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
    new_name: String,
    overwrite: bool,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileRename",
        json!({
            "rootPath": root_path,
            "relativePath": relative_path,
            "newName": new_name,
            "overwrite": overwrite
        }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_delete(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileDelete",
        json!({ "rootPath": root_path, "relativePath": relative_path }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_copy(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    source_path: String,
    target_parent_path: String,
    name: String,
    overwrite: bool,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileCopy",
        json!({
            "rootPath": root_path,
            "sourcePath": source_path,
            "targetParentPath": target_parent_path,
            "name": name,
            "overwrite": overwrite
        }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_move(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    source_path: String,
    target_parent_path: String,
    name: String,
    overwrite: bool,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileMove",
        json!({
            "rootPath": root_path,
            "sourcePath": source_path,
            "targetParentPath": target_parent_path,
            "name": name,
            "overwrite": overwrite
        }),
    )
    .await
}

fn split_relative_file(path: &str) -> Result<(String, String), String> {
    let normalized = path.trim().trim_start_matches('/');
    if normalized.is_empty() {
        return Err("remote_file_path_invalid".to_string());
    }
    match normalized.rsplit_once('/') {
        Some((parent, name)) => Ok((parent.to_string(), name.to_string())),
        None => Ok((String::new(), normalized.to_string())),
    }
}

#[tauri::command]
pub async fn ssh_remote_file_write(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
    content: String,
) -> Result<Value, String> {
    if content.len() <= ATTACHMENT_CHUNK_BYTES {
        return request(
            daemon_bridge,
            consumer_id,
            ssh_launch,
            "fileWrite",
            json!({
                "rootPath": root_path,
                "relativePath": relative_path,
                "content": content
            }),
        )
        .await;
    }
    let (parent_path, name) = split_relative_file(&relative_path)?;
    validate_plan(&ssh_launch)?;
    let client = daemon_bridge
        .get()
        .ok_or_else(|| "daemon_unavailable".to_string())?;
    tokio::task::spawn_blocking(move || {
        write_managed_file_bytes(
            client.as_ref(),
            consumer_id,
            ssh_launch,
            root_path,
            parent_path,
            name,
            content.into_bytes(),
            true,
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn ssh_remote_file_stat(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
) -> Result<Value, String> {
    request(
        daemon_bridge,
        consumer_id,
        ssh_launch,
        "fileStat",
        json!({ "rootPath": root_path, "relativePath": relative_path }),
    )
    .await
}

#[tauri::command]
pub async fn ssh_remote_file_read_bytes(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    relative_path: String,
) -> Result<Value, String> {
    validate_plan(&ssh_launch)?;
    let client = daemon_bridge
        .get()
        .ok_or_else(|| "daemon_unavailable".to_string())?;
    tokio::task::spawn_blocking(move || {
        read_managed_file_bytes(
            client.as_ref(),
            consumer_id,
            ssh_launch,
            root_path,
            relative_path,
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn ssh_remote_file_write_bytes(
    daemon_bridge: tauri::State<'_, DaemonBridge>,
    consumer_id: String,
    ssh_launch: SshLaunchPlan,
    root_path: String,
    parent_path: String,
    name: String,
    data_base64: String,
    overwrite: bool,
) -> Result<Value, String> {
    validate_plan(&ssh_launch)?;
    let data = if data_base64.is_empty() {
        Vec::new()
    } else {
        general_purpose::STANDARD
            .decode(data_base64.as_bytes())
            .map_err(|_| "remote_file_bytes_invalid".to_string())?
    };
    if data.len() > MAX_ATTACHMENT_BYTES {
        return Err("remote_file_too_large".to_string());
    }
    let client = daemon_bridge
        .get()
        .ok_or_else(|| "daemon_unavailable".to_string())?;
    tokio::task::spawn_blocking(move || {
        write_managed_file_bytes(
            client.as_ref(),
            consumer_id,
            ssh_launch,
            root_path,
            parent_path,
            name,
            data,
            overwrite,
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{
        can_fallback_to_legacy_image, decode_attachment, is_legacy_image_name, read_attachment,
        validate_attachment_name, validate_remote_attachment_path, MAX_ATTACHMENT_BYTES,
    };
    use base64::{engine::general_purpose, Engine as _};
    use std::fs;

    #[test]
    fn attachment_names_accept_safe_regular_file_names() {
        assert!(validate_attachment_name("shot.PNG").is_ok());
        assert!(validate_attachment_name("notes.txt").is_ok());
        assert!(validate_attachment_name(".env").is_ok());
        assert!(validate_attachment_name("LICENSE").is_ok());
        assert!(is_legacy_image_name("shot.webp"));
        assert!(!is_legacy_image_name("notes.txt"));
        assert!(!can_fallback_to_legacy_image("shot.png", b"not-an-image"));
        assert!(validate_attachment_name("../shot.png").is_err());
        assert!(validate_attachment_name("folder\\shot.png").is_err());
        assert!(validate_attachment_name("../shot.png\n").is_err());
    }

    #[test]
    fn remote_attachment_paths_are_absolute_and_cache_scoped() {
        assert!(validate_remote_attachment_path(
            "/home/dev/.cache/cli-manager-ssh-agent/attachments/session/id.png"
        )
        .is_ok());
        assert!(validate_remote_attachment_path(
            "/srv/xdg-cache/cli-manager-ssh-agent/attachments/session/id.png"
        )
        .is_ok());
        assert!(
            validate_remote_attachment_path("/project/.cli-manager/attachments/id.png").is_err()
        );
        assert!(validate_remote_attachment_path(
            "/home/dev/.cache/cli-manager-ssh-agent/attachments/../secret.png"
        )
        .is_err());
    }

    #[test]
    fn attachment_data_and_local_paths_are_bounded_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("pixel.png");
        image::save_buffer_with_format(
            &path,
            &[0, 0, 0, 0],
            1,
            1,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(can_fallback_to_legacy_image("pixel.png", &bytes));
        let (_, decoded) =
            decode_attachment("pixel.png".into(), general_purpose::STANDARD.encode(&bytes))
                .unwrap();
        assert_eq!(decoded, bytes);
        let (name, loaded) = read_attachment(path.display().to_string()).unwrap();
        assert_eq!(name, "pixel.png");
        assert_eq!(loaded, bytes);

        let text_path = root.path().join("notes.txt");
        fs::write(&text_path, b"hello").unwrap();
        let (name, loaded) = read_attachment(text_path.display().to_string()).unwrap();
        assert_eq!(name, "notes.txt");
        assert_eq!(loaded, b"hello");

        let oversized = root.path().join("oversized.bin");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_ATTACHMENT_BYTES as u64 + 1)
            .unwrap();
        assert_eq!(
            read_attachment(oversized.display().to_string()).unwrap_err(),
            "attachment_too_large"
        );
    }
}
