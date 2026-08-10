//! 统一的路径打开策略。
//!
//! 所有用户可见的文件打开入口都收口到这里：文本类文件优先交给轻量文本编辑器，
//! 目录与其他文件交给系统默认应用。调用方可显式绕过策略，使用系统默认关联。

use std::path::Path;

use crate::proc_ext::SpawnAndReap;

const TEXT_EXTENSIONS: &[&str] = &[
    "json", "jsonl", "yaml", "yml", "toml", "md", "markdown", "txt", "log",
];

fn is_text_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            TEXT_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

/// 按 Monet 策略打开路径；`system_default` 为 true 时显式绕过扩展名策略。
pub fn open_path(path: &Path, system_default: bool) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("路径不可访问 {}: {error}", path.display()))?;

    if metadata.is_dir() || system_default || !is_text_path(path) {
        open_with_system_default(path)
    } else {
        open_with_text_editor(path)
    }
}

/// 目录等明确要求走系统关联的路径入口。
pub fn open_with_system_default(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(path)
        .spawn_and_reap()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    {
        use crate::proc_ext::HideConsole;
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .hide_console()
            .spawn_and_reap()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn_and_reap()
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn open_with_text_editor(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("-e")
        .arg(path)
        .spawn_and_reap()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    {
        use crate::proc_ext::HideConsole;
        std::process::Command::new("notepad.exe")
            .arg(path)
            .hide_console()
            .spawn_and_reap()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn_and_reap()
        .map_err(|error| error.to_string())?;

    Ok(())
}

/// 网页链接与本地路径分离，避免文件扩展名策略误判 URL。
pub fn open_external_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| format!("无效链接: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("仅允许打开 http/https 链接".to_string());
    }

    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(url)
        .spawn_and_reap()
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    {
        use crate::proc_ext::HideConsole;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .hide_console()
            .spawn_and_reap()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn_and_reap()
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_text_path;
    use std::path::Path;

    #[test]
    fn recognizes_config_and_document_text_extensions_case_insensitively() {
        for path in [
            "settings.json",
            "events.JSONL",
            "config.yaml",
            "config.YML",
            "tool.toml",
            "README.md",
            "notes.markdown",
            "output.txt",
            "runner.log",
        ] {
            assert!(is_text_path(Path::new(path)), "{path} should be text");
        }
    }

    #[test]
    fn leaves_other_file_types_to_the_system() {
        for path in ["image.png", "report.pdf", "archive.zip", "script.rs"] {
            assert!(
                !is_text_path(Path::new(path)),
                "{path} should use system default"
            );
        }
    }
}
