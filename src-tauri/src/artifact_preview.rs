use std::fs;
use std::path::{Component, Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;

const MAX_HTML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SVG_BYTES: u64 = 5 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPreview {
    file_name: String,
    kind: &'static str,
    media_type: &'static str,
    size_bytes: u64,
    text: Option<String>,
    data: Option<String>,
}

struct ArtifactFormat {
    kind: &'static str,
    media_type: &'static str,
    max_bytes: u64,
}

fn artifact_format(path: &Path) -> Option<ArtifactFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let format = match extension.as_str() {
        "html" | "htm" => ArtifactFormat {
            kind: "html",
            media_type: "text/html",
            max_bytes: MAX_HTML_BYTES,
        },
        "svg" => ArtifactFormat {
            kind: "svg",
            media_type: "image/svg+xml",
            max_bytes: MAX_SVG_BYTES,
        },
        "gif" => ArtifactFormat {
            kind: "image",
            media_type: "image/gif",
            max_bytes: MAX_IMAGE_BYTES,
        },
        "png" => ArtifactFormat {
            kind: "image",
            media_type: "image/png",
            max_bytes: MAX_IMAGE_BYTES,
        },
        "jpg" | "jpeg" => ArtifactFormat {
            kind: "image",
            media_type: "image/jpeg",
            max_bytes: MAX_IMAGE_BYTES,
        },
        "webp" => ArtifactFormat {
            kind: "image",
            media_type: "image/webp",
            max_bytes: MAX_IMAGE_BYTES,
        },
        _ => return None,
    };
    Some(format)
}

fn resolve_local_file(root: &Path, requested: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("无法访问会话工作目录: {error}"))?;
    if !root.is_dir() {
        return Err("会话工作目录不是文件夹".to_string());
    }

    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let candidate = candidate
        .canonicalize()
        .map_err(|error| format!("文件不存在或无法访问: {error}"))?;
    if !candidate.is_file() {
        return Err("目标不是普通文件".to_string());
    }
    Ok(candidate)
}

fn resolve_local_file_with_fallback(
    root: &Path,
    requested: &Path,
    fallback_root: Option<&Path>,
) -> Result<PathBuf, String> {
    let Some(fallback_root) = fallback_root else {
        return resolve_local_file(root, requested);
    };
    let relative = if requested.is_absolute() {
        relative_within_root(root, requested)?
    } else {
        requested.to_path_buf()
    };
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("文件路径超出原 Worktree 范围".to_string());
    }
    if let Ok(path) = resolve_local_file(root, requested) {
        return Ok(path);
    }
    let fallback_root = fallback_root
        .canonicalize()
        .map_err(|error| format!("无法访问仓库主目录: {error}"))?;
    let candidate = fallback_root
        .join(&relative)
        .canonicalize()
        .map_err(|_| "主目录中未找到对应文件".to_string())?;
    if !candidate.starts_with(&fallback_root) {
        return Err("映射后的文件路径超出仓库主目录".to_string());
    }
    if !candidate.is_file() {
        return Err("主目录中的映射目标不是普通文件".to_string());
    }
    Ok(candidate)
}

#[cfg(not(windows))]
fn relative_within_root(root: &Path, requested: &Path) -> Result<PathBuf, String> {
    requested
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| "文件路径超出原 Worktree 范围".to_string())
}

#[cfg(windows)]
fn relative_within_root(root: &Path, requested: &Path) -> Result<PathBuf, String> {
    let root_components = root.components().collect::<Vec<_>>();
    let requested_components = requested.components().collect::<Vec<_>>();
    if requested_components.len() < root_components.len()
        || !root_components
            .iter()
            .zip(&requested_components)
            .all(|(left, right)| {
                left.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
            })
    {
        return Err("文件路径超出原 Worktree 范围".to_string());
    }
    let mut relative = PathBuf::new();
    for component in &requested_components[root_components.len()..] {
        relative.push(component.as_os_str());
    }
    Ok(relative)
}

fn validate_image_bytes(media_type: &str, bytes: &[u8]) -> bool {
    match media_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        "image/svg+xml" => {
            let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
            prefix.to_ascii_lowercase().contains("<svg")
        }
        _ => false,
    }
}

#[tauri::command]
pub fn read_artifact_preview(
    root: String,
    path: String,
    fallback_root: Option<String>,
) -> Result<ArtifactPreview, String> {
    let resolved = resolve_local_file_with_fallback(
        Path::new(&root),
        Path::new(&path),
        fallback_root.as_deref().map(Path::new),
    )?;
    let format = artifact_format(&resolved).ok_or_else(|| "不支持预览此文件格式".to_string())?;
    let metadata = fs::metadata(&resolved).map_err(|error| error.to_string())?;
    if metadata.len() > format.max_bytes {
        return Err(format!(
            "交付物过大: {:.1}MB（上限 {}MB）",
            metadata.len() as f64 / 1_048_576.0,
            format.max_bytes / 1_048_576,
        ));
    }

    let bytes = fs::read(&resolved).map_err(|error| error.to_string())?;
    let (text, data) = if format.kind == "html" {
        let text = String::from_utf8(bytes).map_err(|_| "HTML 文件不是有效的 UTF-8".to_string())?;
        (Some(text), None)
    } else {
        if !validate_image_bytes(format.media_type, &bytes) {
            return Err("文件内容与扩展名不匹配".to_string());
        }
        (None, Some(STANDARD.encode(bytes)))
    };

    Ok(ArtifactPreview {
        file_name: resolved
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact")
            .to_string(),
        kind: format.kind,
        media_type: format.media_type,
        size_bytes: metadata.len(),
        text,
        data,
    })
}

#[tauri::command]
pub fn open_local_file(
    root: String,
    path: String,
    fallback_root: Option<String>,
) -> Result<(), String> {
    let resolved = resolve_local_file_with_fallback(
        Path::new(&root),
        Path::new(&path),
        fallback_root.as_deref().map(Path::new),
    )?;
    crate::file_opener::open_path(&resolved, false)
}

#[cfg(test)]
mod tests {
    use super::{read_artifact_preview, resolve_local_file};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "monet-artifact-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn resolves_relative_file_inside_workspace() {
        let root = fixture_root("inside");
        fs::create_dir_all(root.join("output")).unwrap();
        fs::write(root.join("output/demo.html"), "<h1>demo</h1>").unwrap();

        let resolved = resolve_local_file(&root, Path::new("output/demo.html")).unwrap();
        assert_eq!(
            resolved,
            root.join("output/demo.html").canonicalize().unwrap()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_paths_outside_workspace() {
        let root = fixture_root("root");
        let outside = fixture_root("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("demo.html");
        fs::write(&outside_file, "<h1>outside</h1>").unwrap();

        let resolved = resolve_local_file(&root, &outside_file).unwrap();
        assert_eq!(resolved, outside_file.canonicalize().unwrap());

        let relative_outside_file = Path::new("..")
            .join(outside.file_name().unwrap())
            .join("demo.html");
        let resolved = resolve_local_file(&root, &relative_outside_file).unwrap();
        assert_eq!(resolved, outside_file.canonicalize().unwrap());

        assert!(read_artifact_preview(
            root.to_string_lossy().into_owned(),
            outside_file.to_string_lossy().into_owned(),
            None,
        )
        .is_ok());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn resolves_symlinks_that_escape_workspace() {
        use std::os::unix::fs::symlink;

        let root = fixture_root("symlink-root");
        let outside = fixture_root("symlink-outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("demo.html"), "<h1>outside</h1>").unwrap();
        symlink(outside.join("demo.html"), root.join("demo.html")).unwrap();

        let resolved = resolve_local_file(&root, Path::new("demo.html")).unwrap();
        assert_eq!(resolved, outside.join("demo.html").canonicalize().unwrap());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn returns_html_as_text() {
        let root = fixture_root("html");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.html"), "<!doctype html><h1>demo</h1>").unwrap();

        let preview = read_artifact_preview(
            root.to_string_lossy().into_owned(),
            "demo.html".to_string(),
            None,
        )
        .unwrap();
        assert_eq!(preview.kind, "html");
        assert_eq!(
            preview.text.as_deref(),
            Some("<!doctype html><h1>demo</h1>")
        );
        assert!(preview.data.is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_mismatched_image_content() {
        let root = fixture_root("mismatch");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("fake.gif"), "not a gif").unwrap();

        let error = read_artifact_preview(
            root.to_string_lossy().into_owned(),
            "fake.gif".to_string(),
            None,
        )
        .unwrap_err();
        assert!(error.contains("内容与扩展名不匹配"));

        fs::remove_dir_all(root).unwrap();
    }
}
