//! 自动化域数据层（v2.4.0）：Hooks 配置读取 + 近 7 天运行统计。
//!
//! - `get_hooks_config`：解析全局与项目级 settings.json 的 hooks 字段
//! - `get_hooks_stats`：扫描近 7 天 JSONL 反推每条 hook 的运行统计
//! - `open_hooks_config`：按 Monet 的统一文件策略打开指定配置文件

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------- 返回结构 ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEntry {
    pub event: String,
    pub matcher: Option<String>,
    pub command: String,
    pub scope: String,
    pub source_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksConfig {
    pub entries: Vec<HookEntry>,
    pub warnings: Vec<String>,
    /// 家目录绝对路径，供前端做 $HOME 归一化匹配
    pub home_path: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub timestamp: String,
    pub exit_code: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStat {
    pub event: String,
    pub command: String,
    pub runs: u32,
    pub failures: u32,
    pub last_run: Option<LastRun>,
}

// ---------- 内部解析辅助 ----------

/// 将配置/执行记录中的 $HOME / ${HOME} 替换为实际家目录路径
fn normalize_command(cmd: &str, home: &str) -> String {
    cmd.replace("${HOME}", home).replace("$HOME", home)
}

/// 解析单个 settings.json，提取 hooks 条目；解析失败返回 Err(path_str)
fn parse_settings_hooks(path: &Path, scope: &str, _home: &str) -> Result<Vec<HookEntry>, String> {
    let text = fs::read_to_string(path)
        .map_err(|_| path.to_string_lossy().to_string())?;
    let root: Value = serde_json::from_str(&text)
        .map_err(|_| path.to_string_lossy().to_string())?;

    let hooks_obj = match root.get("hooks").and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return Ok(vec![]),
    };

    let mut entries = Vec::new();
    for (event, groups_val) in hooks_obj {
        let groups = match groups_val.as_array() {
            Some(a) => a,
            None => continue,
        };
        for group in groups {
            let matcher = group.get("matcher").and_then(|v| v.as_str()).map(String::from);
            let inner = match group.get("hooks").and_then(|v| v.as_array()) {
                Some(a) => a,
                None => continue,
            };
            for hook in inner {
                if hook.get("type").and_then(|v| v.as_str()) != Some("command") {
                    continue;
                }
                let cmd = match hook.get("command").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => continue,
                };
                entries.push(HookEntry {
                    event: event.clone(),
                    matcher: matcher.clone(),
                    command: cmd.to_string(),
                    scope: scope.to_string(),
                    source_path: path.to_string_lossy().to_string(),
                });
            }
        }
    }
    Ok(entries)
}

// ---------- Commands ----------

/// FR-001: 解析全局与各项目 settings.json 的 hooks 字段
#[tauri::command]
pub fn get_hooks_config() -> Result<HooksConfig, String> {
    let home = dirs::home_dir()
        .ok_or_else(|| "无法获取家目录".to_string())?;
    let home_str = home.to_string_lossy().to_string();

    let mut entries: Vec<HookEntry> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // 全局 settings.json
    let global_path = crate::config::claude_root().join("settings.json");
    if global_path.exists() {
        match parse_settings_hooks(&global_path, "全局", &home_str) {
            Ok(mut e) => entries.append(&mut e),
            Err(p) => warnings.push(format!("{} 解析失败", p)),
        }
    }

    // 项目级:目录名经 discovery 共享解析还原真实 cwd(优先采信会话 JSONL,
    // 退回贪心解码)。项目级 settings 位于 <cwd>/.claude/ 下——projects 目录内
    // 不存在 .claude 子目录,旧实现对着 projects 目录找,项目级 hooks 恒扫不到
    let projects_root = crate::config::projects_dir();
    if projects_root.is_dir() {
        let mut cwds: Vec<String> = fs::read_dir(&projects_root)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                let e = e.ok()?;
                if !e.file_type().ok()?.is_dir() { return None; }
                let dir_name = e.file_name().to_str()?.to_string();
                Some(crate::discovery::resolve_project_path(&dir_name))
            })
            .collect();
        // 大小写变体等场景同一 cwd 可能出现多次,去重防 hooks 条目重复
        cwds.sort_unstable();
        cwds.dedup();

        for cwd in &cwds {
            // 项目 scope 显示为 cwd 的最后一段目录名
            let scope = Path::new(cwd)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(cwd.as_str())
                .to_string();

            for file_name in &["settings.json", "settings.local.json"] {
                let settings_path = Path::new(cwd).join(".claude").join(file_name);
                if settings_path.exists() {
                    match parse_settings_hooks(&settings_path, &scope, &home_str) {
                        Ok(mut e) => entries.append(&mut e),
                        Err(p) => warnings.push(format!("{} 解析失败", p)),
                    }
                }
            }
        }
    }

    // 排序：全局 → 项目字典序，同组内按 event 字典序
    entries.sort_by(|a, b| {
        let scope_ord = if a.scope == "全局" { 0 } else { 1 }
            .cmp(&if b.scope == "全局" { 0 } else { 1 });
        scope_ord
            .then_with(|| a.scope.cmp(&b.scope))
            .then_with(|| a.event.cmp(&b.event))
    });

    Ok(HooksConfig { entries, warnings, home_path: home_str })
}

// ---------- Hooks 统计 ----------

#[derive(Deserialize)]
struct AttachmentRecord {
    #[serde(rename = "type")]
    record_type: Option<String>,
    attachment: Option<AttachmentField>,
    timestamp: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentField {
    #[serde(rename = "type")]
    att_type: Option<String>,
    hook_event: Option<String>,
    command: Option<String>,
    exit_code: Option<Value>,
}

/// 聚合 key = (hookEvent, command_normalized)
#[derive(Debug, Eq, PartialEq, Hash)]
struct StatKey {
    event: String,
    command: String,
}

struct StatAccum {
    runs: u32,
    failures: u32,
    last_run: Option<(String, i64)>, // (timestamp, exitCode)
}

/// 7 天窗口的截止时间（SystemTime）
fn seven_days_ago() -> Option<SystemTime> {
    SystemTime::now().checked_sub(Duration::from_secs(7 * 24 * 3600))
}

/// FR-002: 扫描近 7 天 JSONL，聚合 hook 运行统计
#[tauri::command]
pub fn get_hooks_stats() -> Result<Vec<HookStat>, String> {
    let home = dirs::home_dir()
        .ok_or_else(|| "无法获取家目录".to_string())?;
    let home_str = home.to_string_lossy().to_string();
    let projects_root = crate::config::projects_dir();

    let cutoff = seven_days_ago();
    // 7 天前的 Unix 时间戳（秒），用于 ISO timestamp 精筛
    let cutoff_ts_secs = cutoff
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    if !projects_root.is_dir() {
        return Ok(vec![]);
    }

    // 收集近 7 天（按文件 mtime 粗筛）的 .jsonl 文件路径
    let mut jsonl_paths: Vec<PathBuf> = Vec::new();
    collect_jsonl_files(&projects_root, cutoff, &mut jsonl_paths);

    // rayon 并行扫描，各线程返回局部聚合 map
    let partial_maps: Vec<HashMap<StatKey, StatAccum>> = jsonl_paths
        .par_iter()
        .map(|path| scan_jsonl_file(path, cutoff_ts_secs, &home_str))
        .collect();

    // 合并所有局部 map
    let mut merged: HashMap<StatKey, StatAccum> = HashMap::new();
    for partial in partial_maps {
        for (key, acc) in partial {
            let entry = merged.entry(key).or_insert(StatAccum {
                runs: 0,
                failures: 0,
                last_run: None,
            });
            entry.runs += acc.runs;
            entry.failures += acc.failures;
            if let Some((ts, code)) = acc.last_run {
                let should_update = match &entry.last_run {
                    None => true,
                    Some((existing_ts, _)) => ts > *existing_ts,
                };
                if should_update {
                    entry.last_run = Some((ts, code));
                }
            }
        }
    }

    let stats: Vec<HookStat> = merged
        .into_iter()
        .map(|(key, acc)| HookStat {
            event: key.event,
            command: key.command,
            runs: acc.runs,
            failures: acc.failures,
            last_run: acc.last_run.map(|(ts, code)| LastRun {
                timestamp: ts,
                exit_code: code,
            }),
        })
        .collect();

    Ok(stats)
}

/// 递归收集满足 mtime 粗筛的 .jsonl 文件（跳过 journal.jsonl）
fn collect_jsonl_files(dir: &Path, cutoff: Option<SystemTime>, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.filter_map(|e| e.ok()) {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            collect_jsonl_files(&path, cutoff, out);
        } else if ft.is_file() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.ends_with(".jsonl") || name_str == "journal.jsonl" {
                continue;
            }
            // mtime 粗筛
            if let Some(cutoff_time) = cutoff {
                if let Ok(meta) = fs::metadata(&path) {
                    if let Ok(mtime) = meta.modified() {
                        if mtime < cutoff_time {
                            continue;
                        }
                    }
                }
            }
            out.push(path);
        }
    }
}

/// 扫描单个 JSONL 文件，返回局部聚合 map
fn scan_jsonl_file(
    path: &Path,
    cutoff_ts_secs: Option<u64>,
    home: &str,
) -> HashMap<StatKey, StatAccum> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let mut map: HashMap<StatKey, StatAccum> = HashMap::new();

    for line in text.lines() {
        if line.trim().is_empty() { continue; }
        let rec: AttachmentRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // 只处理顶层 type == "attachment"
        if rec.record_type.as_deref() != Some("attachment") { continue; }

        let att = match &rec.attachment {
            Some(a) => a,
            None => continue,
        };

        // attachment.type 以 hook_ 开头
        let att_type = match &att.att_type {
            Some(t) => t.as_str(),
            None => continue,
        };
        if !att_type.starts_with("hook_") { continue; }

        // 必须有 exitCode
        let exit_code = match &att.exit_code {
            Some(v) => {
                if let Some(n) = v.as_i64() { n }
                else { continue; }
            }
            None => continue,
        };

        let event = match &att.hook_event {
            Some(e) => e.clone(),
            None => continue,
        };
        let command = match &att.command {
            Some(c) => normalize_command(c, home),
            None => continue,
        };

        // timestamp 精筛：若存在且早于 7 天则跳过
        if let Some(cutoff_secs) = cutoff_ts_secs {
            if let Some(ts_str) = &rec.timestamp {
                if let Ok(secs) = parse_iso_to_secs(ts_str) {
                    if secs < cutoff_secs { continue; }
                }
            }
        }

        let key = StatKey { event, command };
        let acc = map.entry(key).or_insert(StatAccum {
            runs: 0,
            failures: 0,
            last_run: None,
        });
        acc.runs += 1;
        if exit_code != 0 { acc.failures += 1; }

        // lastRun：取 timestamp 最大的一条
        if let Some(ts) = &rec.timestamp {
            let should_update = match &acc.last_run {
                None => true,
                Some((existing, _)) => ts > existing,
            };
            if should_update {
                acc.last_run = Some((ts.clone(), exit_code));
            }
        }
    }

    map
}

/// 将 ISO 8601 时间戳解析为 Unix 秒（简单字符串比较够用，但精筛需要数值）
fn parse_iso_to_secs(ts: &str) -> Result<u64, ()> {
    // 利用 chrono 不可引入，用简单字节解析：2026-06-12T08:43:06.812Z
    // 只需秒精度，取前 19 字符 "2026-06-12T08:43:06"
    if ts.len() < 19 { return Err(()); }
    let parts: Vec<&str> = ts[..19].split('T').collect();
    if parts.len() != 2 { return Err(()); }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    let time_parts: Vec<&str> = parts[1].split(':').collect();
    if date_parts.len() != 3 || time_parts.len() != 3 { return Err(()); }

    let year: i64 = date_parts[0].parse().map_err(|_| ())?;
    let month: i64 = date_parts[1].parse().map_err(|_| ())?;
    let day: i64 = date_parts[2].parse().map_err(|_| ())?;
    let hour: i64 = time_parts[0].parse().map_err(|_| ())?;
    let min: i64 = time_parts[1].parse().map_err(|_| ())?;
    let sec: i64 = time_parts[2].parse().map_err(|_| ())?;

    // 粗略转换（忽略闰年/闰秒，精度够用于 7 天窗口判断）
    let days_from_epoch = days_since_epoch(year, month, day);
    let secs = days_from_epoch * 86400 + hour * 3600 + min * 60 + sec;
    Ok(secs.max(0) as u64)
}

fn days_since_epoch(year: i64, month: i64, day: i64) -> i64 {
    // 参考 https://howardhinnant.github.io/date_algorithms.html civil_from_days 逆运算
    let m = month;
    let y = if m <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

// ---------- 打开配置 ----------

/// FR-005: 按统一策略打开指定配置文件路径
#[tauri::command]
pub fn open_hooks_config(path: String, system_default: Option<bool>) -> Result<(), String> {
    crate::file_opener::open_path(
        std::path::Path::new(&path),
        system_default.unwrap_or(false),
    )
}
