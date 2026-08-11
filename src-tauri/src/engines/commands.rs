use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use super::core::*;
use super::system::{self, EngineInitializationError};

const MAX_PAGE_REQUESTS: usize = 10_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineListResult {
    pub engines: Vec<EngineDescriptor>,
    pub initialization_errors: Vec<EngineInitializationError>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSearchQuery {
    pub text: String,
    pub instance: Option<EngineInstanceId>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSearchHit {
    pub session: SessionRef,
    pub title: Option<String>,
    pub snippet: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineDiagnosticEntry {
    descriptor: EngineDescriptor,
    health: Option<EngineHealth>,
    error: Option<EngineError>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineDiagnosticReport {
    generated_at: String,
    app_version: &'static str,
    engines: Vec<EngineDiagnosticEntry>,
}

#[tauri::command]
pub fn engine_list() -> Result<EngineListResult, EngineError> {
    let system = system::get()?;
    Ok(EngineListResult {
        engines: system.registry().descriptors(),
        initialization_errors: system
            .initialization_errors()
            .iter()
            .cloned()
            .map(|mut error| {
                error.message = sanitize_diagnostic(&error.message);
                error
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn engine_health(instance: EngineInstanceId) -> EngineResult<EngineHealth> {
    let registry = system::get()?.registry();
    let descriptor = registry.descriptor(&instance)?;
    if !descriptor.enabled {
        return Ok(disabled_health(descriptor));
    }
    registry.adapter(&instance)?.health().await
}

#[tauri::command]
pub fn engine_set_enabled(instance: EngineInstanceId, enabled: bool) -> EngineResult<()> {
    system::get()?.registry().descriptor(&instance)?;
    super::preferences::set_enabled(&instance, enabled)
}

#[tauri::command]
pub fn engine_open_configuration(
    instance: EngineInstanceId,
    system_default: Option<bool>,
) -> EngineResult<()> {
    let path = system::get()?
        .registry()
        .adapter(&instance)?
        .configuration_path()
        .ok_or_else(|| unsupported("configuration file"))?;

    if !path.exists() {
        crate::config::atomic_write(&path, "{}\n")
            .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))?;
    }

    crate::file_opener::open_path(&path, system_default.unwrap_or(false))
        .map_err(|error| EngineError::new(EngineErrorKind::Io, error))
}

#[tauri::command]
pub async fn engine_export_diagnostics(path: PathBuf) -> EngineResult<()> {
    let system = system::get()?;
    let mut engines = Vec::new();
    for descriptor in system.registry().descriptors() {
        let result = if descriptor.enabled {
            match system.registry().adapter(&descriptor.instance) {
                Ok(adapter) => adapter.health().await,
                Err(error) => Err(error),
            }
        } else {
            Ok(disabled_health(descriptor.clone()))
        };
        let (health, error) = match result {
            Ok(health) => (Some(sanitize_health(health)), None),
            Err(mut error) => {
                error.message = sanitize_diagnostic(&error.message);
                (None, Some(error))
            }
        };
        engines.push(EngineDiagnosticEntry {
            descriptor,
            health,
            error,
        });
    }
    let bytes = serde_json::to_vec_pretty(&EngineDiagnosticReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION"),
        engines,
    })
    .map_err(|error| EngineError::new(EngineErrorKind::Internal, error.to_string()))?;
    std::fs::write(path, bytes)
        .map_err(|error| EngineError::new(EngineErrorKind::Io, error.to_string()))
}

#[tauri::command]
pub async fn engine_list_projects(
    instance: EngineInstanceId,
    query: ProjectQuery,
) -> EngineResult<ProjectPage> {
    system::get()?
        .registry()
        .adapter(&instance)?
        .session_source()
        .list_projects(query)
        .await
}

#[tauri::command]
pub async fn engine_list_sessions(
    project: ProjectRef,
    query: SessionQuery,
) -> EngineResult<SessionPage> {
    let mut page = system::get()?
        .registry()
        .adapter(project.engine())?
        .session_source()
        .list_sessions(project, query)
        .await?;
    page.sessions.retain(|summary| {
        !crate::metadata::metadata_for_ref(&summary.reference)
            .and_then(|metadata| metadata.deleted)
            .unwrap_or(false)
    });
    Ok(page)
}

#[tauri::command]
pub async fn engine_load_timeline(
    session: SessionRef,
    page: TimelinePage,
) -> EngineResult<ConversationPage> {
    system::get()?
        .registry()
        .source_for(&session)?
        .load_timeline(session, page)
        .await
}

#[tauri::command]
pub async fn engine_session_actions(session: SessionRef) -> EngineResult<SessionActions> {
    system::get()?
        .registry()
        .source_for(&session)?
        .session_actions(session)
        .await
}

#[tauri::command]
pub async fn engine_resolve_asset(
    asset: AssetRef,
    preview: Option<bool>,
) -> EngineResult<ResolvedAsset> {
    const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;
    let mut resolved = system::get()?
        .registry()
        .source_for(&asset.session)?
        .resolve_asset(asset)
        .await?;
    if resolved.bytes.len() > MAX_ASSET_BYTES {
        return Err(EngineError::new(
            EngineErrorKind::Protocol,
            "resolved asset exceeds the transfer size limit",
        ));
    }
    if preview.unwrap_or(false) {
        if let Some((media_type, bytes)) =
            crate::image_protocol::make_engine_thumbnail(&resolved.media_type, &resolved.bytes)
        {
            resolved = ResolvedAsset { media_type, bytes };
        }
    }
    Ok(resolved)
}

#[tauri::command]
pub async fn engine_search(query: EngineSearchQuery) -> EngineResult<Vec<EngineSearchHit>> {
    let system = system::get()?;
    let needles: Vec<_> = query
        .text
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|value| !value.is_empty())
        .collect();
    if needles.is_empty() {
        return Ok(Vec::new());
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let descriptors = system.registry().descriptors();
    let mut hits = Vec::new();
    let mut successful_engines = 0_usize;
    let mut first_error = None;

    for descriptor in descriptors {
        if !descriptor.enabled {
            continue;
        }
        if query
            .instance
            .as_ref()
            .is_some_and(|instance| instance != &descriptor.instance)
        {
            continue;
        }
        let remaining = limit - hits.len();
        let result: EngineResult<Vec<EngineSearchHit>> = async {
            let source = system
                .registry()
                .adapter(&descriptor.instance)?
                .session_source();
            let projects = collect_projects(source).await?;
            let mut engine_hits = Vec::new();
            for project in projects {
                let project_reference = project.reference;
                let mut sessions = collect_sessions(source, project_reference.clone()).await?;
                sessions.retain(|session| {
                    !crate::metadata::metadata_for_ref(&session.reference)
                        .and_then(|metadata| metadata.deleted)
                        .unwrap_or(false)
                });
                let documents =
                    super::search::documents_for_project(source, project_reference, sessions)
                        .await?;
                for mut document in documents {
                    let metadata = crate::metadata::metadata_for_ref(&document.session);
                    if let Some(title) = metadata.as_ref().and_then(|value| value.title.clone()) {
                        document.title = Some(title);
                    }
                    let searchable = format!(
                        "{}\n{}\n{}\n{}",
                        document.title.as_deref().unwrap_or_default(),
                        document.text,
                        metadata
                            .as_ref()
                            .and_then(|value| value.tags.as_ref())
                            .map(|values| values.join(" "))
                            .unwrap_or_default(),
                        metadata
                            .as_ref()
                            .and_then(|value| value.summary.as_deref())
                            .unwrap_or_default(),
                    );
                    let positions: Option<Vec<_>> = needles
                        .iter()
                        .map(|needle| folded_match_position(&searchable, needle))
                        .collect();
                    let Some(position) = positions.and_then(|values| values.into_iter().min())
                    else {
                        continue;
                    };
                    engine_hits.push(EngineSearchHit {
                        session: document.session,
                        title: document.title,
                        snippet: text_window(&searchable, position),
                    });
                    if engine_hits.len() >= remaining {
                        return Ok(engine_hits);
                    }
                }
            }
            Ok(engine_hits)
        }
        .await;
        match result {
            Ok(engine_hits) => {
                successful_engines += 1;
                hits.extend(engine_hits);
                if hits.len() >= limit {
                    return Ok(hits);
                }
            }
            Err(error) if query.instance.is_some() => return Err(error),
            Err(error) => {
                log::warn!(
                    "engine search failed engineId={} instanceId={} errorKind={:?}",
                    descriptor.instance.engine_id(),
                    descriptor.instance.instance_id(),
                    error.kind,
                );
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if successful_engines == 0 {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(hits)
}

fn disabled_health(descriptor: EngineDescriptor) -> EngineHealth {
    EngineHealth {
        instance: descriptor.instance,
        status: EngineHealthStatus::Disabled,
        installed: false,
        authenticated: None,
        version: None,
        version_supported: None,
        executable_path: None,
        source: CapabilityHealth::unavailable("engine.disabled"),
        runtime: CapabilityHealth::unavailable("engine.disabled"),
        diagnostics: Vec::new(),
    }
}

fn sanitize_health(mut health: EngineHealth) -> EngineHealth {
    health.executable_path = health.executable_path.as_deref().and_then(|path| {
        std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
    });
    for diagnostic in &mut health.diagnostics {
        diagnostic.message = sanitize_diagnostic(&diagnostic.message);
    }
    health
}

fn sanitize_diagnostic(message: &str) -> String {
    let redacted =
        if let Some(home) = dirs::home_dir().and_then(|path| path.to_str().map(str::to_string)) {
            message.replace(&home, "~")
        } else {
            message.to_string()
        };
    redacted.chars().take(512).collect()
}

#[tauri::command]
pub async fn engine_list_assets(
    instance: EngineInstanceId,
    query: FacetQuery,
) -> EngineResult<FacetPage> {
    let adapter = system::get()?.registry().adapter_arc(&instance)?;
    adapter
        .assets()
        .ok_or_else(|| unsupported("assets"))?
        .list_assets(query)
        .await
}

#[tauri::command]
pub async fn engine_read_quota(
    instance: EngineInstanceId,
    force_refresh: bool,
) -> EngineResult<Value> {
    let adapter = system::get()?.registry().adapter_arc(&instance)?;
    adapter
        .quota()
        .ok_or_else(|| unsupported("quota"))?
        .read_quota(force_refresh)
        .await
}

#[tauri::command]
pub async fn engine_list_runtime_commands(session: SessionRef) -> EngineResult<Vec<FacetItem>> {
    let adapter = system::get()?.registry().adapter_arc(session.engine())?;
    adapter
        .runtime_commands()
        .ok_or_else(|| unsupported("runtimeCommands"))?
        .list_commands(session)
        .await
}

#[tauri::command]
pub async fn engine_list_models(instance: EngineInstanceId) -> EngineResult<Vec<ModelDescriptor>> {
    let adapter = system::get()?.registry().adapter_arc(&instance)?;
    adapter
        .models()
        .ok_or_else(|| unsupported("models"))?
        .list_models()
        .await
}

#[tauri::command]
pub async fn engine_create_session(request: CreateSessionRequest) -> EngineResult<RuntimeSession> {
    system::get()?.coordinator().create_session(request).await
}

#[tauri::command]
pub async fn engine_fork_session(request: ForkSessionRequest) -> EngineResult<RuntimeSession> {
    system::get()?.coordinator().fork_session(request).await
}

#[tauri::command]
pub async fn engine_attach_session(
    session: SessionRef,
    options: AttachOptions,
) -> EngineResult<RuntimeSession> {
    system::get()?
        .coordinator()
        .attach_session(session, options)
        .await
}

#[tauri::command]
pub async fn engine_start_turn(
    session: SessionRef,
    request: TurnRequest,
) -> EngineResult<TurnHandle> {
    system::get()?
        .coordinator()
        .start_turn(session, request)
        .await
}

#[tauri::command]
pub async fn engine_send_input_while_running(
    turn: TurnRef,
    input: Vec<InputItem>,
) -> EngineResult<()> {
    system::get()?
        .coordinator()
        .send_input_while_running(turn, input)
        .await
}

#[tauri::command]
pub async fn engine_interrupt_turn(turn: TurnRef) -> EngineResult<()> {
    system::get()?.coordinator().interrupt_turn(turn).await
}

#[tauri::command]
pub async fn engine_respond_interaction(
    request: InteractionRef,
    response: InteractionResponse,
) -> EngineResult<()> {
    system::get()?
        .coordinator()
        .respond(request, response)
        .await
}

#[tauri::command]
pub async fn engine_close_session(session: SessionRef) -> EngineResult<()> {
    system::get()?.coordinator().close_session(session).await
}

#[tauri::command]
pub fn engine_runtime_snapshots() -> Result<Vec<RuntimeSnapshot>, EngineError> {
    Ok(system::get()?.coordinator().reconcile_snapshots())
}

async fn collect_projects(source: &dyn SessionSource) -> EngineResult<Vec<CoreProject>> {
    let mut projects = Vec::new();
    let mut cursor = None;
    for _ in 0..MAX_PAGE_REQUESTS {
        let page = source
            .list_projects(ProjectQuery {
                cursor: cursor.clone(),
                limit: Some(200),
            })
            .await?;
        projects.extend(page.projects);
        if page.next_cursor.is_none() || page.next_cursor == cursor {
            return Ok(projects);
        }
        cursor = page.next_cursor;
    }
    Err(EngineError::new(
        EngineErrorKind::Protocol,
        "project pagination exceeded the safety limit",
    ))
}

async fn collect_sessions(
    source: &dyn SessionSource,
    project: ProjectRef,
) -> EngineResult<Vec<CoreSessionSummary>> {
    let mut sessions = Vec::new();
    let mut cursor = None;
    for _ in 0..MAX_PAGE_REQUESTS {
        let page = source
            .list_sessions(
                project.clone(),
                SessionQuery {
                    cursor: cursor.clone(),
                    limit: Some(200),
                },
            )
            .await?;
        sessions.extend(page.sessions);
        if page.next_cursor.is_none() || page.next_cursor == cursor {
            return Ok(sessions);
        }
        cursor = page.next_cursor;
    }
    Err(EngineError::new(
        EngineErrorKind::Protocol,
        "session pagination exceeded the safety limit",
    ))
}

fn unsupported(facet: &str) -> EngineError {
    EngineError::new(
        EngineErrorKind::Unsupported,
        format!("engine does not provide the {facet} facet"),
    )
}

fn folded_match_position(text: &str, needle: &str) -> Option<usize> {
    let mut folded = String::with_capacity(text.len());
    let mut original_positions = Vec::with_capacity(text.len());
    for (position, character) in text.char_indices() {
        let part = character.to_lowercase().to_string();
        original_positions.extend(std::iter::repeat(position).take(part.len()));
        folded.push_str(&part);
    }
    folded
        .find(needle)
        .and_then(|position| original_positions.get(position).copied())
}

fn text_window(text: &str, byte_position: usize) -> String {
    let start = text[..byte_position]
        .char_indices()
        .rev()
        .nth(80)
        .map(|(position, _)| position)
        .unwrap_or(0);
    let end = text[byte_position..]
        .char_indices()
        .nth(120)
        .map(|(position, _)| byte_position + position)
        .unwrap_or(text.len());
    text[start..end].replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_remain_on_utf8_boundaries() {
        let text = "前文 对话中的关键字 后文";
        let byte_position = text.find("关键字").unwrap();
        assert!(text_window(text, byte_position).contains("关键字"));
    }

    #[test]
    fn folded_search_maps_case_expansion_back_to_utf8_boundaries() {
        let text = "İstanbul 中的记录";
        let position = folded_match_position(text, "i̇stanbul").unwrap();
        assert_eq!(&text[position..position + "İstanbul".len()], "İstanbul");
    }
}
