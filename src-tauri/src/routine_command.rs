//! Routine CLI 命令构造的单一事实源。
//!
//! 主 App 的「立即运行」与独立 `monet-routine-runner` 都直接编译本文件，
//! 保证两条执行路径对引擎、持久化和环境变量的解释完全一致。本文件只能依赖
//! std 与 routine_types，不能引用 Tauri 或 app_lib 专属状态。

use std::path::Path;
use std::process::{Command, Stdio};

use crate::routine_types::RoutineEngine;

pub struct RoutineCommandSpec<'a> {
    pub engine: &'a RoutineEngine,
    pub executable: &'a Path,
    pub prompt: &'a str,
    pub session_id: &'a str,
    pub persist_session: bool,
    pub cwd: &'a Path,
    pub path_env: &'a str,
    pub claude_config_dir: Option<&'a str>,
    pub codex_home: Option<&'a str>,
}

pub fn build_routine_command(spec: RoutineCommandSpec<'_>) -> Result<Command, String> {
    let mut command = Command::new(spec.executable);
    command.env("PATH", spec.path_env);

    if spec.engine.is_claude_code() {
        command.env_remove("MONET_CLAUDE_ROOT");
        command.env_remove("CLAUDE_CONFIG_DIR");
        if let Some(directory) = spec.claude_config_dir {
            command.env("CLAUDE_CONFIG_DIR", directory);
        }
        command
            .arg("-p")
            .arg(spec.prompt)
            .arg("--output-format")
            .arg("text")
            .arg("--session-id")
            .arg(spec.session_id);
        if !spec.persist_session {
            command.arg("--no-session-persistence");
        }
    } else if spec.engine.is_codex() {
        command.env_remove("CODEX_HOME");
        if let Some(directory) = spec.codex_home {
            command.env("CODEX_HOME", directory);
        }
        command
            .arg("exec")
            .arg("--json")
            .arg("--skip-git-repo-check")
            .arg("--color")
            .arg("never")
            .arg("--cd")
            .arg(spec.cwd);
        if !spec.persist_session {
            command.arg("--ephemeral");
        }
        // `--` 保证以连字符开头的自然语言仍被当作 prompt，而不是 CLI 参数。
        command.arg("--").arg(spec.prompt);
    } else {
        return Err(format!(
            "unsupported routine engine: {}/{}",
            spec.engine.engine_id, spec.engine.instance_id
        ));
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    command
        .current_dir(spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    fn spec(engine: &RoutineEngine, persist_session: bool) -> RoutineCommandSpec<'_> {
        RoutineCommandSpec {
            engine,
            executable: Path::new("/usr/bin/example"),
            prompt: "- summarize",
            session_id: "session-id",
            persist_session,
            cwd: Path::new("/workspace/agent"),
            path_env: "/usr/bin",
            claude_config_dir: Some("/config/claude"),
            codex_home: Some("/config/codex"),
        }
    }

    #[test]
    fn builds_claude_command_with_explicit_session() {
        let engine = RoutineEngine::claude_code();
        let command = build_routine_command(spec(&engine, true)).unwrap();

        assert_eq!(
            args(&command),
            [
                "-p",
                "- summarize",
                "--output-format",
                "text",
                "--session-id",
                "session-id",
            ]
        );
        assert!(command
            .get_envs()
            .any(|(key, value)| key == OsStr::new("CLAUDE_CONFIG_DIR")
                && value == Some(OsStr::new("/config/claude"))));
    }

    #[test]
    fn builds_codex_command_and_maps_no_persistence_to_ephemeral() {
        let engine = RoutineEngine::codex();
        let command = build_routine_command(spec(&engine, false)).unwrap();

        assert_eq!(
            args(&command),
            [
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--color",
                "never",
                "--cd",
                "/workspace/agent",
                "--ephemeral",
                "--",
                "- summarize",
            ]
        );
        assert!(command
            .get_envs()
            .any(|(key, value)| key == OsStr::new("CODEX_HOME")
                && value == Some(OsStr::new("/config/codex"))));
    }

    #[test]
    fn rejects_unimplemented_engine_instances() {
        let engine = RoutineEngine {
            engine_id: "codex".into(),
            instance_id: "secondary".into(),
        };
        assert!(build_routine_command(spec(&engine, true))
            .unwrap_err()
            .contains("unsupported routine engine"));
    }
}
