use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

use crate::channels::{resolve_agent_for_feature, AgentChannelCredentials};
use crate::config;
use crate::proc_ext::HideConsole;
use crate::streaming::{enhanced_path, find_claude};

fn locales_dir() -> PathBuf {
    config::data_dir().join("locales")
}

fn build_translate_prompt(source_json: &str, target_lang: &str, target_native: &str) -> String {
    format!(
        "Translate the following JSON locale file from Chinese (zh-CN) to {target_lang} ({target_native}).\n\n\
        Rules:\n\
        - Translate ONLY the string values, keep all keys exactly the same\n\
        - Keep {{variable}} placeholders like {{count}}, {{name}}, {{n}} unchanged\n\
        - Keep technical terms (Claude, Codex, API, JSON, MCP, Tauri, JSONL, CLI, Token, Remote Control, etc.) unchanged\n\
        - Keep symbols like ✓, ✗, ⚠, ▲, ▼, → unchanged\n\
        - Output ONLY valid JSON — no markdown fences, no explanation\n\
        - The translation should feel natural in {target_lang}\n\n\
        Source JSON:\n{source_json}"
    )
}

fn strip_markdown_fence(text: &str) -> &str {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```json") {
        rest.trim().strip_suffix("```").unwrap_or(rest.trim())
    } else if let Some(rest) = t.strip_prefix("```") {
        rest.trim().strip_suffix("```").unwrap_or(rest.trim())
    } else {
        t
    }
}

#[derive(Default, Clone)]
pub(crate) struct ApiUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

fn extract_usage(v: &Value) -> Option<ApiUsage> {
    let u = v.get("usage")?;
    Some(ApiUsage {
        input_tokens: u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        output_tokens: u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
    })
}

fn extract_usage_openai(v: &Value) -> Option<ApiUsage> {
    let u = v.get("usage")?;
    Some(ApiUsage {
        input_tokens: u.get("prompt_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        output_tokens: u.get("completion_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
    })
}

#[allow(dead_code)] // 供渠道翻译使用的便捷入口
pub(crate) fn http_call_messages(
    base_url: &str,
    token: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> Result<String, String> {
    http_call_messages_with_usage(base_url, token, prompt, model, max_tokens).map(|(text, _)| text)
}

pub(crate) fn http_call_messages_with_usage(
    base_url: &str,
    token: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> Result<(String, Option<ApiUsage>), String> {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": false,
        "messages": [{"role": "user", "content": prompt}]
    });

    let resp = client
        .post(&url)
        .header("x-api-key", token)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .map_err(|e| format!("API 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().unwrap_or_default();
        return Err(format!("API {} — {}", status, body_text));
    }

    let body_text = resp.text().map_err(|e| format!("响应读取失败: {}", e))?;
    let resp_json: Value = serde_json::from_str(&body_text)
        .map_err(|e| format!("响应解析失败: {} — body: {}", e, &body_text[..body_text.len().min(200)]))?;
    let usage = extract_usage(&resp_json);
    let text = resp_json
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "API 响应格式异常".to_string())?;
    Ok((text, usage))
}

#[allow(dead_code)] // 供 OpenAI 协议渠道翻译使用的便捷入口
pub(crate) fn http_call_openai(
    base_url: &str,
    token: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> Result<String, String> {
    http_call_openai_with_usage(base_url, token, prompt, model, max_tokens).map(|(text, _)| text)
}

pub(crate) fn http_call_openai_with_usage(
    base_url: &str,
    token: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> Result<(String, Option<ApiUsage>), String> {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": false,
        "messages": [{"role": "user", "content": prompt}]
    });

    let mut req = client
        .post(&url)
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(120));

    if !token.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", token));
    }

    let resp = req
        .json(&body)
        .send()
        .map_err(|e| format!("API 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().unwrap_or_default();
        return Err(format!("API {} — {}", status, body_text));
    }

    let body_text = resp.text().map_err(|e| format!("响应读取失败: {}", e))?;
    let resp_json: Value = serde_json::from_str(&body_text)
        .map_err(|e| format!("响应解析失败: {} — body: {}", e, &body_text[..body_text.len().min(200)]))?;
    let usage = extract_usage_openai(&resp_json);
    let text = resp_json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "API 响应格式异常".to_string())?;
    Ok((text, usage))
}

pub(crate) fn http_call_by_protocol(
    base_url: &str,
    token: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
    protocol: &str,
) -> Result<String, String> {
    http_call_by_protocol_with_usage(base_url, token, prompt, model, max_tokens, protocol)
        .map(|(text, _)| text)
}

pub(crate) fn http_call_by_protocol_with_usage(
    base_url: &str,
    token: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
    protocol: &str,
) -> Result<(String, Option<ApiUsage>), String> {
    match protocol {
        "openai" => http_call_openai_with_usage(base_url, token, prompt, model, max_tokens),
        _ => http_call_messages_with_usage(base_url, token, prompt, model, max_tokens),
    }
}



fn translate_via_cli(
    source_json: &str,
    target_lang: &str,
    target_native: &str,
) -> Result<String, String> {
    let prompt = build_translate_prompt(source_json, target_lang, target_native);
    let (executable, prefix_args) = find_claude();
    let mut args = prefix_args;
    args.extend([
        "-p".to_string(),
        "--model".to_string(),
        // alias 抗版本漂移(原带日期 ID 本身可疑),且可被渠道 ANTHROPIC_DEFAULT_SONNET_MODEL 模型映射重定向
        "sonnet".to_string(),
        "--output-format".to_string(),
        "text".to_string(),
    ]);
    if !crate::channels::agent_session_persist() {
        args.push("--no-session-persistence".to_string());
    }

    let output = Command::new(&executable)
        .args(&args)
        .current_dir(crate::config::agent_cwd())
        .env("PATH", enhanced_path())
        .hide_console()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(prompt.as_bytes());
            }
            drop(child.stdin.take());
            child.wait_with_output()
        })
        .map_err(|e| format!("CLI 启动失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("CLI 退出 {}: {}", output.status, stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn do_translate(
    cred: &AgentChannelCredentials,
    source_json: &str,
    target_lang: &str,
    target_native: &str,
) -> Result<String, String> {
    if cred.is_official {
        translate_via_cli(source_json, target_lang, target_native)
    } else {
        let prompt = build_translate_prompt(source_json, target_lang, target_native);
        let model = cred.agent_model.as_deref().unwrap_or("claude-sonnet-4-6-20250514");
        http_call_by_protocol(
            cred.base_url.as_deref().unwrap(),
            cred.token.as_deref().unwrap_or(""),
            &prompt, model, 16000,
            &cred.protocol,
        )
    }
}

fn save_locale(
    lang_code: &str,
    target_lang: &str,
    target_native: &str,
    translated: &Value,
) -> Result<String, String> {
    let dir = locales_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;

    let pretty = serde_json::to_string_pretty(translated).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(dir.join(format!("{}.json", lang_code)), &pretty)
        .map_err(|e| format!("写入失败: {}", e))?;

    let meta = json!({ "code": lang_code, "label": target_lang, "nativeLabel": target_native });
    let meta_str = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("{}.meta.json", lang_code)), &meta_str)
        .map_err(|e| format!("写入失败: {}", e))?;

    Ok(pretty)
}

#[tauri::command]
pub async fn translate_locale(
    source_json: String,
    target_lang: String,
    target_native: String,
    lang_code: String,
) -> Result<String, String> {
    if !crate::channels::is_agent_enabled("translate") {
        return Err("agent.translate 已禁用".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        eprintln!("[translate] target={} ({}) code={}", target_lang, target_native, lang_code);

        let raw = if crate::channels::default_agent_engine() == "codex" {
            let prompt = build_translate_prompt(&source_json, &target_lang, &target_native);
            crate::agent::request_for_agent(&prompt, "translate")?
        } else if let Some(cred) = resolve_agent_for_feature("translate") {
            do_translate(&cred, &source_json, &target_lang, &target_native)?
        } else {
            translate_via_cli(&source_json, &target_lang, &target_native)?
        };

        let clean = strip_markdown_fence(&raw);
        let translated: Value =
            serde_json::from_str(clean).map_err(|e| format!("翻译结果不是有效 JSON: {}", e))?;
        save_locale(&lang_code, &target_lang, &target_native, &translated)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn parse_language_intent(user_input: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let prompt = format!(
            "【角色：语言识别器】用户想添加一种界面语言。根据输入识别目标语言。\
            输出纯 JSON（不要 markdown），格式：\
            {{\"code\":\"ja-JP\",\"name\":\"Japanese\",\"native\":\"日本語\"}}\n\n\
            规则：code 用 IETF 格式（如 ja-JP, ko-KR, fr-FR）。\
            如果无法识别，输出 {{\"error\":\"无法识别\"}}\n\n\
            用户输入：{}",
            user_input
        );
        let result = crate::agent::request_blocking_pub(&prompt)?;
        let clean = strip_markdown_fence(&result);
        serde_json::from_str(clean).map_err(|e| format!("解析失败: {}", e))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_external_locales() -> Result<Vec<Value>, String> {
    let dir = locales_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".meta.json") { continue; }
        let code = name.trim_end_matches(".meta.json");
        let meta: Value = fs::read_to_string(entry.path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let messages: Value = fs::read_to_string(dir.join(format!("{}.json", code)))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}));
        result.push(json!({
            "code": code,
            "label": meta.get("label").and_then(|v| v.as_str()).unwrap_or(code),
            "nativeLabel": meta.get("nativeLabel").and_then(|v| v.as_str()).unwrap_or(code),
            "messages": messages,
        }));
    }
    Ok(result)
}

#[tauri::command]
pub fn delete_external_locale(code: String) -> Result<(), String> {
    let dir = locales_dir();
    let _ = fs::remove_file(dir.join(format!("{}.json", code)));
    let _ = fs::remove_file(dir.join(format!("{}.meta.json", code)));
    Ok(())
}
