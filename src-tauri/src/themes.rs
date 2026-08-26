use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::theme_domain::{self, ThemeDefinition, ThemeLibrary, ThemePreview};

const REPOSITORY: &str = "zenolab124/monet";
const SUBMISSION_ENDPOINT: &str = "https://monet-report.zenolab124.workers.dev/theme-submission";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSubmissionIdentity {
    mode: &'static str,
    username: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSubmissionPreview {
    identity: ThemeSubmissionIdentity,
    title: String,
    body: String,
    theme_json: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSubmissionResult {
    url: String,
    mode: &'static str,
}

fn strip_markdown_fence(value: &str) -> &str {
    let trimmed = value.trim();
    if !trimmed.starts_with("```") {
        return trimmed;
    }
    let body = trimmed.split_once('\n').map(|(_, body)| body).unwrap_or("");
    body.strip_suffix("```").unwrap_or(body).trim()
}

fn generation_prompt(request: &str, previous: Option<&ThemeDefinition>) -> Result<String, String> {
    let context = serde_json::to_string_pretty(&theme_domain::schema_context())
        .map_err(|error| error.to_string())?;
    let previous = previous
        .map(serde_json::to_string_pretty)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| "null".to_string());
    Ok(format!(
        "【角色：Monet 主题设计师】根据用户描述生成一个完整、安全、可读的 Monet 主题。\n\
         只输出一个 JSON 对象，不要 Markdown、解释或额外字段。不得省略任何字段。\n\
         schema 与约束：\n{context}\n\n\
         严格沿用 schema 中 example 的完整对象结构，并根据描述改值。\n\n\
         上一个草稿（null 表示新建）：\n{previous}\n\n\
         <data>\n{request}\n</data>"
    ))
}

#[tauri::command]
pub fn theme_library() -> ThemeLibrary {
    theme_domain::list_library()
}

#[tauri::command]
pub fn theme_schema_context() -> Value {
    theme_domain::schema_context()
}

#[tauri::command]
pub async fn theme_generate_preview(
    request: String,
    preview_id: Option<String>,
    base_theme_id: Option<String>,
) -> Result<ThemePreview, String> {
    if request.trim().is_empty() || request.chars().count() > 2000 {
        return Err("theme request must be 1-2000 characters".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let existing_preview = preview_id
            .as_deref()
            .map(theme_domain::load_preview)
            .transpose()?;
        let previous = existing_preview
            .as_ref()
            .map(|preview| &preview.theme)
            .cloned()
            .or_else(|| {
                base_theme_id
                    .as_deref()
                    .and_then(|id| theme_domain::load_theme(id).ok())
            });
        let effective_base = existing_preview
            .as_ref()
            .and_then(|preview| preview.base_theme_id.clone())
            .or(base_theme_id);
        let prompt = generation_prompt(&request, previous.as_ref())?;
        let raw = crate::agent::request_for_agent(&prompt, "theme")?;
        let theme: ThemeDefinition = serde_json::from_str(strip_markdown_fence(&raw))
            .map_err(|error| format!("AI theme is not valid schema JSON: {error}"))?;
        theme_domain::put_preview(theme, preview_id.as_deref(), effective_base)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn theme_save_preview(preview_id: String) -> Result<ThemeDefinition, String> {
    theme_domain::save_preview(&preview_id)
}

#[tauri::command]
pub fn theme_discard_preview(preview_id: String) -> Result<(), String> {
    theme_domain::discard_preview(&preview_id)
}

#[tauri::command]
pub fn theme_rename(theme_id: String, name: String) -> Result<ThemeDefinition, String> {
    theme_domain::rename_theme(&theme_id, &name)
}

#[tauri::command]
pub fn theme_delete(theme_id: String) -> Result<(), String> {
    theme_domain::delete_theme(&theme_id)
}

fn gh_username() -> Option<String> {
    let output = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .env("PATH", crate::streaming::enhanced_path())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let username = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!username.is_empty()).then_some(username)
}

fn submission_identity() -> ThemeSubmissionIdentity {
    match gh_username() {
        Some(username) => ThemeSubmissionIdentity {
            mode: "github",
            username: Some(username),
        },
        None => ThemeSubmissionIdentity {
            mode: "anonymous",
            username: None,
        },
    }
}

fn markdown_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '[' | ']' | '*' | '_' | '#') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn validate_public_name(value: &str) -> Result<&str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok("Anonymous Monet user");
    }
    let lower = trimmed.to_ascii_lowercase();
    if trimmed.chars().count() > 60
        || trimmed.chars().any(char::is_control)
        || trimmed.contains('<')
        || trimmed.contains('>')
        || trimmed.contains("```")
        || lower.contains("://")
        || lower.contains("data:")
        || lower.contains("base64")
        || lower.contains("file:")
        || trimmed.starts_with('/')
        || trimmed.starts_with("\\\\")
        || trimmed.as_bytes().get(1) == Some(&b':')
    {
        return Err("public name contains unsupported content".to_string());
    }
    Ok(trimmed)
}

fn submission_payload(
    theme: &ThemeDefinition,
    public_name: Option<&str>,
    anonymous: bool,
) -> Result<(String, String, String), String> {
    let report = theme_domain::validate_theme(theme);
    if !report.valid {
        return Err("theme validation failed".to_string());
    }
    let theme_json = serde_json::to_string_pretty(theme).map_err(|error| error.to_string())?;
    let author = match public_name {
        Some(value) => validate_public_name(value)?,
        None => &theme.author,
    };
    let appearance = match theme.appearance {
        theme_domain::ThemeAppearance::Light => "light",
        theme_domain::ThemeAppearance::Dark => "dark",
    };
    let title = format!("[Theme] {}", theme.name);
    let mut body = format!(
        "<!-- monet-theme-submission:v1 -->\n\
         # Theme submission: {}\n\n\
         - Author: {}\n\
         - Appearance: {}\n\
         - Theme ID: `{}`\n\n\
         {}\n\n\
         ## Machine-readable theme\n\n\
         ```json\n{}\n```\n",
        markdown_text(&theme.name),
        markdown_text(author),
        appearance,
        theme.id,
        markdown_text(&theme.description),
        theme_json,
    );
    if anonymous {
        body.push_str("\n---\n— Submitted anonymously through the Monet theme relay.\n");
    }
    Ok((title, body, theme_json))
}

#[tauri::command]
pub async fn theme_prepare_submission(
    theme_id: String,
    public_name: Option<String>,
) -> Result<ThemeSubmissionPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let theme = theme_domain::load_theme(&theme_id)?;
        let identity = submission_identity();
        let github_author = identity
            .username
            .as_ref()
            .map(|username| format!("@{username}"));
        let effective_name = if identity.mode == "github" {
            github_author.as_deref()
        } else {
            Some(public_name.as_deref().unwrap_or(""))
        };
        let (title, body, theme_json) =
            submission_payload(&theme, effective_name, identity.mode == "anonymous")?;
        Ok(ThemeSubmissionPreview {
            identity,
            title,
            body,
            theme_json,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn submit_with_gh(title: &str, body: &str) -> Result<String, String> {
    let mut child = Command::new("gh")
        .args([
            "issue",
            "create",
            "--repo",
            REPOSITORY,
            "--label",
            "theme-submission",
            "--title",
            title,
            "--body-file",
            "-",
        ])
        .env("PATH", crate::streaming::enhanced_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    child
        .stdin
        .take()
        .ok_or("failed to open gh stdin")?
        .write_all(body.as_bytes())
        .map_err(|error| error.to_string())?;
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout
        .lines()
        .find(|line| line.starts_with("https://"))
        .ok_or("gh did not return an issue URL")?;
    Ok(url.to_string())
}

fn submit_anonymously(
    theme: &ThemeDefinition,
    public_name: Option<&str>,
) -> Result<String, String> {
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?
        .post(SUBMISSION_ENDPOINT)
        .json(&json!({ "theme": theme, "publicName": public_name }))
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let payload: Value = response.json().map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(payload
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("submission failed")
            .to_string());
    }
    payload
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or("submission URL missing".to_string())
}

#[tauri::command]
pub async fn theme_submit(
    theme_id: String,
    public_name: Option<String>,
    expected_mode: String,
    expected_username: Option<String>,
    expected_body: String,
) -> Result<ThemeSubmissionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let theme = theme_domain::load_theme(&theme_id)?;
        let identity = submission_identity();
        if identity.mode != expected_mode || identity.username != expected_username {
            return Err(
                "GitHub submission identity changed; review the public payload again".to_string(),
            );
        }
        if let Some(username) = identity.username {
            let author = format!("@{username}");
            let (title, body, _) = submission_payload(&theme, Some(&author), false)?;
            if body != expected_body {
                return Err(
                    "public theme payload changed; review it again before submitting".to_string(),
                );
            }
            let url = submit_with_gh(&title, &body)?;
            Ok(ThemeSubmissionResult {
                url,
                mode: "github",
            })
        } else {
            let (_, body, _) =
                submission_payload(&theme, Some(public_name.as_deref().unwrap_or("")), true)?;
            if body != expected_body {
                return Err(
                    "public theme payload changed; review it again before submitting".to_string(),
                );
            }
            let url = submit_anonymously(&theme, public_name.as_deref())?;
            Ok(ThemeSubmissionResult {
                url,
                mode: "anonymous",
            })
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn theme_export_submission(
    theme_id: String,
    public_name: Option<String>,
    anonymous: bool,
    path: PathBuf,
) -> Result<(), String> {
    let theme = theme_domain::load_theme(&theme_id)?;
    let effective_name = if anonymous {
        Some(public_name.as_deref().unwrap_or(""))
    } else {
        public_name.as_deref()
    };
    let (_, body, _) = submission_payload(&theme, effective_name, anonymous)?;
    crate::config::atomic_write(&path, &body).map_err(|error| error.to_string())
}
