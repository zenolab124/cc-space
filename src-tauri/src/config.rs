use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static CLAUDE_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn data_dir() -> &'static PathBuf {
    DATA_DIR.get_or_init(|| {
        if let Ok(dir) = std::env::var("MONET_DATA_DIR") {
            PathBuf::from(dir)
        } else {
            let home = dirs::home_dir().unwrap_or_default();
            let target = home.join(".monet");
            // 数据目录更名迁移(cc-space → monet):新目录尚未建立且旧目录仍在时，
            // 整体 rename 搬迁;失败则回退沿用旧目录(不丢数据,仅打日志)。
            // OnceLock 初始化只跑一次,迁移检测无高频代价。
            if !target.exists() {
                let legacy = home.join(".cc-space");
                if legacy.is_dir() {
                    if let Err(e) = std::fs::rename(&legacy, &target) {
                        // 本文件经 #[path] 编进精简 bin(monet-mcp),坚持 std-only,不引 log
                        eprintln!("[monet] 数据目录迁移 ~/.cc-space → ~/.monet 失败,继续沿用旧目录: {e}");
                        return legacy;
                    }
                }
            }
            target
        }
    })
}

/// 展开路径开头的 `~`（仅 `~` 与 `~/...` 两种形式）
fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    PathBuf::from(path)
}

/// 读取 ~/.monet/settings.json 中的单个设置键（散读，不依赖 typed struct）
pub fn read_app_setting(key: &str) -> Option<serde_json::Value> {
    let path = data_dir().join("settings.json");
    let content = std::fs::read_to_string(path).ok()?;
    let settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&content).ok()?;
    settings.get(key).cloned()
}

/// 写入 ~/.monet/settings.json 中的单个设置键；value 为 null 时删除该键。
/// 文件存在但解析失败时拒绝覆写，避免带着空 map 清掉其他设置键
pub fn write_app_setting(key: &str, value: serde_json::Value) {
    let _ = write_app_setting_checked(key, value);
}

pub fn write_app_setting_checked(key: &str, value: serde_json::Value) -> Result<(), String> {
    let path = data_dir().join("settings.json");
    let mut settings: serde_json::Map<String, serde_json::Value> =
        match std::fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(m) => m,
                Err(error) => return Err(format!("设置文件解析失败: {error}")),
            },
            Err(_) => Default::default(),
        };
    if value.is_null() {
        settings.remove(key);
    } else {
        settings.insert(key.to_string(), value);
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(settings))
        .map_err(|error| format!("设置序列化失败: {error}"))?;
    atomic_write(&path, &json).map_err(|error| format!("设置写入失败: {error}"))
}

/// Claude Code 数据根目录（默认 ~/.claude）的**唯一**解析入口。
/// 全项目禁止再手拼 `home.join(".claude")` —— 一律经此收口。
///
/// 优先级：
/// 1. env `MONET_CLAUDE_ROOT`（进程级临时覆盖，不落设置）
/// 2. env `CLAUDE_CONFIG_DIR`（Claude CLI 官方变量，终端启动时对齐 CLI 行为）
/// 3. 设置项 `claudeRoot`（~/.monet/settings.json，App 内可配）
/// 4. 默认 `~/.claude`
///
/// 进程级缓存（OnceLock）：watcher 与搜索索引均为启动时一次性绑定，
/// 改设置后统一「重启生效」，缓存与该语义自洽
pub fn claude_root() -> &'static PathBuf {
    CLAUDE_ROOT.get_or_init(resolve_claude_root)
}

/// 即时解析（绕过进程级缓存）：设置页用它对比「当前生效」与「重启后生效」，
/// 判断是否需要提示重启。业务代码一律用 claude_root()，不要用这个
pub fn resolve_claude_root() -> PathBuf {
    for var in ["MONET_CLAUDE_ROOT", "CLAUDE_CONFIG_DIR"] {
        if let Ok(dir) = std::env::var(var) {
            let dir = dir.trim();
            if !dir.is_empty() {
                return expand_tilde(dir);
            }
        }
    }
    if let Some(v) = read_app_setting("claudeRoot") {
        if let Some(s) = v.as_str() {
            let s = s.trim();
            if !s.is_empty() {
                return expand_tilde(s);
            }
        }
    }
    default_claude_root()
}

/// 默认 Claude 数据根（不含任何覆盖），供设置 UI 展示与「是否默认」判断
pub fn default_claude_root() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude")
}

/// Claude Code 用户级 JSON 配置文件路径。
pub fn claude_settings_path() -> PathBuf {
    claude_root().join("settings.json")
}

/// 会话 JSONL 所在的 projects 根目录
pub fn projects_dir() -> PathBuf {
    claude_root().join("projects")
}

/// 顶层 `.claude.json` 的位置：默认根时位于 `~/.claude.json`（home 下），
/// 自定义根时随 CLAUDE_CONFIG_DIR 官方语义位于 `<root>/.claude.json`
pub fn claude_json_path() -> PathBuf {
    let root = claude_root();
    if is_default_claude_root(root) {
        dirs::home_dir().unwrap_or_default().join(".claude.json")
    } else {
        root.join(".claude.json")
    }
}

fn is_default_claude_root(root: &Path) -> bool {
    *root == default_claude_root()
}

/// spawn claude CLI 时的数据根对齐：根非默认时返回需注入的
/// `CLAUDE_CONFIG_DIR`，让子进程 CLI 与 Monet 读取侧看到同一数据根
/// （否则自定义根下 Monet 发起的会话会落到默认 ~/.claude，自己读不到）。
/// 默认根时返回 None，零扰动
pub fn claude_config_dir_env() -> Option<(&'static str, String)> {
    let root = claude_root();
    if is_default_claude_root(root) {
        None
    } else {
        Some(("CLAUDE_CONFIG_DIR", root.display().to_string()))
    }
}

/// 内置 Agent 的专属工作目录：spawn CLI 时的 cwd 固定在这里，
/// 与用户项目隔离（Agent 调用另有 --no-session-persistence 保证不落盘）
pub fn agent_cwd() -> PathBuf {
    let p = data_dir().join("agent");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// Claude CLI 对 cwd 的 projects 目录编码：非字母数字字符一律替换为 `-`
/// （CLI 实现为 `replace(/[^a-zA-Z0-9]/g, "-")`，正则无 `u` 标志、按 UTF-16 code unit
/// 逐个替换：astral 字符（emoji 等代理对）产出 2 个 `-`，此处用 encode_utf16 对齐该语义；
/// Windows 盘符 `C:\` 因此编码为 `C--`）。
/// Windows canonicalize 会带 verbatim 前缀（`\\?\C:\...` 与 `\\?\UNC\server\...`），
/// CLI 侧无此前缀，编码前剥离对齐（UNC 形态还原为 `\\server\...`）
pub fn encode_project_dir(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    };
    s.encode_utf16()
        .map(|u| match u {
            0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A => u as u8 as char,
            _ => '-',
        })
        .collect()
}

/// 内置 Agent 工作目录对应的 projects 编码名（含历史数据目录）。
/// Agent 会话默认落盘（设置项 agentSessionPersist 可关，供事后追溯）；
/// 这份清单是全部扫描面的软屏蔽口径：档案馆/搜索/watcher/用量统计/Widget
/// 据此排除 Agent 会话目录，不混入用户视图
pub fn agent_project_dirs() -> Vec<String> {
    // CLI 对 cwd 先 canonicalize 再编码（symlink 场景逻辑路径会对不上），对齐口径；
    // canonicalize 失败（目录尚不存在等）回落逻辑路径
    let cwd = agent_cwd();
    let canonical = cwd.canonicalize().unwrap_or(cwd);
    let mut names = vec![encode_project_dir(&canonical)];
    if let Some(home) = dirs::home_dir() {
        // 数据目录迁移前的旧位置 ~/.claude/cc-space/agent
        let legacy = home.join(".claude").join("cc-space").join("agent");
        let legacy = legacy.canonicalize().unwrap_or(legacy);
        names.push(encode_project_dir(&legacy));
    }
    names.dedup();
    names
}

/// 原子写文本文件（临时文件 + rename）。
/// settings.json 等被主 App 与 runner 跨进程读写的文件必须走这里，
/// 裸 fs::write 的 truncate-write 间隙会被并发读者读到半截 JSON
pub fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn encode_unix_path() {
        // 非字母数字一律 → '-'：'/'、'.'、'_' 同待遇（对齐 CLI 规则）
        assert_eq!(
            encode_project_dir(Path::new("/Users/alice/my.app_v2")),
            "-Users-alice-my-app-v2"
        );
    }

    #[test]
    fn encode_windows_drive_path() {
        assert_eq!(
            encode_project_dir(Path::new(r"C:\Users\alice\workspace\monet")),
            "C--Users-alice-workspace-monet"
        );
    }

    #[test]
    fn encode_strips_verbatim_prefix() {
        // canonicalize 在 Windows 产生 \\?\ 前缀，CLI 侧无此前缀
        assert_eq!(
            encode_project_dir(Path::new(r"\\?\C:\Users\alice")),
            "C--Users-alice"
        );
    }

    #[test]
    fn encode_strips_unc_verbatim_prefix() {
        // \\?\UNC\server\share 是 \\server\share 的 verbatim 形态，须还原后编码
        assert_eq!(
            encode_project_dir(Path::new(r"\\?\UNC\server\share\proj")),
            "--server-share-proj"
        );
    }

    #[test]
    fn encode_astral_char_as_two_dashes() {
        // CLI 正则无 u 标志、按 UTF-16 code unit 替换：代理对（emoji）产出 2 个 '-'
        assert_eq!(
            encode_project_dir(Path::new("/Users/alice/🚀proj")),
            "-Users-alice---proj"
        );
        // BMP 内非 ASCII（中文）单 code unit，一比一
        assert_eq!(encode_project_dir(Path::new("/tmp/项目")), "-tmp---");
    }
}
