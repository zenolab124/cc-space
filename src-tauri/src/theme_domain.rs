use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const THEME_SCHEMA_VERSION: u32 = 1;
const RESERVED_IDS: [&str; 2] = ["paper", "ink"];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeAppearance {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeSourceKind {
    Local,
    Community,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeSource {
    pub kind: ThemeSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeColors {
    pub background: String,
    pub foreground: String,
    pub card: String,
    pub card_foreground: String,
    pub popover: String,
    pub popover_foreground: String,
    pub primary: String,
    pub primary_foreground: String,
    pub secondary: String,
    pub secondary_foreground: String,
    pub muted: String,
    pub muted_foreground: String,
    pub accent: String,
    pub accent_foreground: String,
    pub destructive: String,
    pub destructive_foreground: String,
    pub border: String,
    pub input: String,
    pub ring: String,
    pub claude: String,
    pub codex: String,
    pub tag: String,
    pub tag_foreground: String,
    pub visual_border: String,
    pub visual_warm: String,
    pub visual_cool: String,
    pub visual_red: String,
    pub visual_green: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeShadow {
    pub color: String,
    pub opacity: f32,
    pub y: u8,
    pub blur: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeMetrics {
    pub radius: u8,
    pub font_scale: f32,
    pub line_height: f32,
    pub shadow: ThemeShadow,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeAtmosphere {
    pub tint: String,
    pub noise: f32,
    pub vignette: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeDefinition {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub appearance: ThemeAppearance,
    pub source: ThemeSource,
    pub colors: ThemeColors,
    pub metrics: ThemeMetrics,
    pub atmosphere: ThemeAtmosphere,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeValidationIssue {
    pub field: String,
    pub message: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeValidationReport {
    pub valid: bool,
    pub issues: Vec<ThemeValidationIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemePreview {
    pub preview_id: String,
    pub theme: ThemeDefinition,
    pub validation: ThemeValidationReport,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_theme_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeLibrary {
    pub themes: Vec<ThemeDefinition>,
    pub previews: Vec<ThemePreview>,
    pub invalid_entries: Vec<String>,
}

fn themes_dir() -> PathBuf {
    crate::config::data_dir().join("themes")
}

fn previews_dir() -> PathBuf {
    crate::config::data_dir().join("theme-previews")
}

fn theme_path(id: &str) -> PathBuf {
    themes_dir().join(format!("{id}.json"))
}

fn safe_theme_id(id: &str) -> bool {
    id.len() >= 3
        && id.len() <= 40
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn ensure_theme_id(id: &str) -> Result<(), String> {
    safe_theme_id(id)
        .then_some(())
        .ok_or_else(|| "invalid theme id".to_string())
}

fn ensure_preview_id(id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "invalid preview id".to_string())
}

fn preview_path(id: &str) -> PathBuf {
    previews_dir().join(format!("{id}.json"))
}

fn issue(field: &str, kind: &str, message: impl Into<String>) -> ThemeValidationIssue {
    ThemeValidationIssue {
        field: field.to_string(),
        kind: kind.to_string(),
        message: message.into(),
    }
}

fn validate_text(field: &str, value: &str, max: usize, issues: &mut Vec<ThemeValidationIssue>) {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        issues.push(issue(
            field,
            "schema",
            format!("must be 1-{max} characters"),
        ));
        return;
    }
    let lower = trimmed.to_ascii_lowercase();
    let unsafe_content = trimmed.chars().any(char::is_control)
        || trimmed.contains('<')
        || trimmed.contains('>')
        || trimmed.contains("```")
        || lower.contains("://")
        || lower.contains("data:")
        || lower.contains("base64")
        || lower.contains("file:")
        || trimmed.starts_with('/')
        || trimmed.starts_with("\\\\")
        || trimmed.as_bytes().get(1) == Some(&b':');
    if unsafe_content {
        issues.push(issue(
            field,
            "unsafe",
            "contains a URL, path, markup, or control sequence",
        ));
    }
}

fn parse_hex(value: &str) -> Option<[u8; 3]> {
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    Some([
        u8::from_str_radix(&value[1..3], 16).ok()?,
        u8::from_str_radix(&value[3..5], 16).ok()?,
        u8::from_str_radix(&value[5..7], 16).ok()?,
    ])
}

fn luminance(rgb: [u8; 3]) -> f64 {
    let linear = |value: u8| {
        let channel = f64::from(value) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

fn contrast(a: &str, b: &str) -> Option<f64> {
    let first = luminance(parse_hex(a)?);
    let second = luminance(parse_hex(b)?);
    let (light, dark) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    Some((light + 0.05) / (dark + 0.05))
}

pub fn validate_theme(theme: &ThemeDefinition) -> ThemeValidationReport {
    let mut issues = Vec::new();
    if theme.schema_version != THEME_SCHEMA_VERSION {
        issues.push(issue(
            "schemaVersion",
            "schema",
            format!("must equal {THEME_SCHEMA_VERSION}"),
        ));
    }
    if !safe_theme_id(&theme.id) || RESERVED_IDS.contains(&theme.id.as_str()) {
        issues.push(issue(
            "id",
            "schema",
            "must be a non-reserved lowercase slug (3-40 characters)",
        ));
    }
    if theme.source.kind != ThemeSourceKind::Local || theme.source.issue.is_some() {
        issues.push(issue(
            "source",
            "schema",
            "local drafts must use { kind: local } without an issue",
        ));
    }
    validate_text("name", &theme.name, 60, &mut issues);
    validate_text("author", &theme.author, 60, &mut issues);
    validate_text("description", &theme.description, 500, &mut issues);
    validate_text("version", &theme.version, 20, &mut issues);

    let colors = &theme.colors;
    let values = [
        ("colors.background", &colors.background),
        ("colors.foreground", &colors.foreground),
        ("colors.card", &colors.card),
        ("colors.cardForeground", &colors.card_foreground),
        ("colors.popover", &colors.popover),
        ("colors.popoverForeground", &colors.popover_foreground),
        ("colors.primary", &colors.primary),
        ("colors.primaryForeground", &colors.primary_foreground),
        ("colors.secondary", &colors.secondary),
        ("colors.secondaryForeground", &colors.secondary_foreground),
        ("colors.muted", &colors.muted),
        ("colors.mutedForeground", &colors.muted_foreground),
        ("colors.accent", &colors.accent),
        ("colors.accentForeground", &colors.accent_foreground),
        ("colors.destructive", &colors.destructive),
        (
            "colors.destructiveForeground",
            &colors.destructive_foreground,
        ),
        ("colors.border", &colors.border),
        ("colors.input", &colors.input),
        ("colors.ring", &colors.ring),
        ("colors.claude", &colors.claude),
        ("colors.codex", &colors.codex),
        ("colors.tag", &colors.tag),
        ("colors.tagForeground", &colors.tag_foreground),
        ("colors.visualBorder", &colors.visual_border),
        ("colors.visualWarm", &colors.visual_warm),
        ("colors.visualCool", &colors.visual_cool),
        ("colors.visualRed", &colors.visual_red),
        ("colors.visualGreen", &colors.visual_green),
        ("metrics.shadow.color", &theme.metrics.shadow.color),
        ("atmosphere.tint", &theme.atmosphere.tint),
    ];
    for (field, value) in values {
        if parse_hex(value).is_none() {
            issues.push(issue(field, "schema", "must be a #RRGGBB color"));
        }
    }

    let pairs = [
        (
            "foreground/background",
            &colors.foreground,
            &colors.background,
        ),
        ("cardForeground/card", &colors.card_foreground, &colors.card),
        (
            "popoverForeground/popover",
            &colors.popover_foreground,
            &colors.popover,
        ),
        (
            "primaryForeground/primary",
            &colors.primary_foreground,
            &colors.primary,
        ),
        (
            "secondaryForeground/secondary",
            &colors.secondary_foreground,
            &colors.secondary,
        ),
        (
            "mutedForeground/muted",
            &colors.muted_foreground,
            &colors.muted,
        ),
        (
            "accentForeground/accent",
            &colors.accent_foreground,
            &colors.accent,
        ),
        (
            "destructiveForeground/destructive",
            &colors.destructive_foreground,
            &colors.destructive,
        ),
        ("tagForeground/tag", &colors.tag_foreground, &colors.tag),
    ];
    for (field, foreground, background) in pairs {
        if let Some(ratio) = contrast(foreground, background) {
            if ratio < 4.5 {
                issues.push(issue(
                    field,
                    "contrast",
                    format!("contrast {ratio:.2}:1 is below WCAG AA 4.5:1"),
                ));
            }
        }
    }

    let metrics = &theme.metrics;
    if !(2..=16).contains(&metrics.radius) {
        issues.push(issue("metrics.radius", "range", "must be between 2 and 16"));
    }
    if !metrics.font_scale.is_finite() || !(0.90..=1.15).contains(&metrics.font_scale) {
        issues.push(issue(
            "metrics.fontScale",
            "range",
            "must be between 0.90 and 1.15",
        ));
    }
    if !metrics.line_height.is_finite() || !(1.30..=1.90).contains(&metrics.line_height) {
        issues.push(issue(
            "metrics.lineHeight",
            "range",
            "must be between 1.30 and 1.90",
        ));
    }
    if !metrics.shadow.opacity.is_finite() || !(0.0..=0.6).contains(&metrics.shadow.opacity) {
        issues.push(issue(
            "metrics.shadow.opacity",
            "range",
            "must be between 0 and 0.6",
        ));
    }
    if metrics.shadow.y > 16 || metrics.shadow.blur > 40 {
        issues.push(issue(
            "metrics.shadow",
            "range",
            "shadow y/blur exceed the allowed range",
        ));
    }
    if !theme.atmosphere.noise.is_finite() || !(0.0..=0.12).contains(&theme.atmosphere.noise) {
        issues.push(issue(
            "atmosphere.noise",
            "range",
            "must be between 0 and 0.12",
        ));
    }
    if !theme.atmosphere.vignette.is_finite() || !(0.0..=0.30).contains(&theme.atmosphere.vignette)
    {
        issues.push(issue(
            "atmosphere.vignette",
            "range",
            "must be between 0 and 0.30",
        ));
    }
    ThemeValidationReport {
        valid: issues.is_empty(),
        issues,
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    crate::config::atomic_write(path, &format!("{content}\n")).map_err(|error| error.to_string())
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

pub fn list_library() -> ThemeLibrary {
    let mut invalid_entries = Vec::new();
    let mut themes = Vec::new();
    for path in json_files(&themes_dir()) {
        match read_json::<ThemeDefinition>(&path) {
            Ok(theme)
                if validate_theme(&theme).valid
                    && path.file_stem().and_then(|value| value.to_str())
                        == Some(theme.id.as_str()) =>
            {
                themes.push(theme)
            }
            _ => invalid_entries.push(
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            ),
        }
    }
    let mut previews = Vec::new();
    for path in json_files(&previews_dir()) {
        match read_json::<ThemePreview>(&path) {
            Ok(mut preview)
                if path.file_stem().and_then(|value| value.to_str())
                    == Some(preview.preview_id.as_str()) =>
            {
                preview.validation = validate_theme(&preview.theme);
                previews.push(preview);
            }
            _ => invalid_entries.push(
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            ),
        }
    }
    themes.sort_by_key(|theme| theme.name.to_lowercase());
    previews.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    ThemeLibrary {
        themes,
        previews,
        invalid_entries,
    }
}

pub fn load_theme(id: &str) -> Result<ThemeDefinition, String> {
    ensure_theme_id(id)?;
    let theme: ThemeDefinition = read_json(&theme_path(id))?;
    if theme.id != id {
        return Err("theme id does not match its file".to_string());
    }
    let report = validate_theme(&theme);
    if report.valid {
        Ok(theme)
    } else {
        Err("theme is invalid".to_string())
    }
}

pub fn load_preview(id: &str) -> Result<ThemePreview, String> {
    ensure_preview_id(id)?;
    let mut preview: ThemePreview = read_json(&preview_path(id))?;
    if preview.preview_id != id {
        return Err("preview id does not match its file".to_string());
    }
    preview.validation = validate_theme(&preview.theme);
    Ok(preview)
}

pub fn put_preview(
    theme: ThemeDefinition,
    preview_id: Option<&str>,
    base_theme_id: Option<String>,
) -> Result<ThemePreview, String> {
    let id = preview_id
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    ensure_preview_id(&id)?;
    if preview_id.is_some() && !preview_path(&id).exists() {
        return Err("preview not found".to_string());
    }
    if let Some(base_id) = base_theme_id.as_deref() {
        let base = load_theme(base_id)?;
        if theme.id != base.id {
            return Err("an adjusted theme must keep its original id".to_string());
        }
    }
    let preview = ThemePreview {
        validation: validate_theme(&theme),
        preview_id: id.clone(),
        theme,
        created_at: Utc::now().to_rfc3339(),
        base_theme_id,
    };
    write_json(&preview_path(&id), &preview)?;
    Ok(preview)
}

pub fn discard_preview(id: &str) -> Result<(), String> {
    ensure_preview_id(id)?;
    let path = preview_path(id);
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn save_preview(id: &str) -> Result<ThemeDefinition, String> {
    let preview = load_preview(id)?;
    let report = validate_theme(&preview.theme);
    if !report.valid {
        return Err("theme validation failed".to_string());
    }
    let path = theme_path(&preview.theme.id);
    if path.exists() && preview.base_theme_id.as_deref() != Some(preview.theme.id.as_str()) {
        return Err("a theme with this id already exists".to_string());
    }
    write_json(&path, &preview.theme)?;
    discard_preview(id)?;
    Ok(preview.theme)
}

pub fn rename_theme(id: &str, name: &str) -> Result<ThemeDefinition, String> {
    ensure_theme_id(id)?;
    let mut theme = load_theme(id)?;
    let mut issues = Vec::new();
    validate_text("name", name, 60, &mut issues);
    if !issues.is_empty() {
        return Err(issues[0].message.clone());
    }
    theme.name = name.trim().to_string();
    write_json(&theme_path(id), &theme)?;
    Ok(theme)
}

pub fn delete_theme(id: &str) -> Result<(), String> {
    ensure_theme_id(id)?;
    if RESERVED_IDS.contains(&id) {
        return Err("built-in themes cannot be deleted".to_string());
    }
    let path = theme_path(id);
    if !path.exists() {
        return Err("theme not found".to_string());
    }
    fs::remove_file(path).map_err(|error| error.to_string())
}

pub fn schema_context() -> serde_json::Value {
    serde_json::json!({
        "schemaVersion": THEME_SCHEMA_VERSION,
        "appearance": ["light", "dark"],
        "colorFormat": "#RRGGBB",
        "requiredColorTokens": [
            "background", "foreground", "card", "cardForeground", "popover", "popoverForeground",
            "primary", "primaryForeground", "secondary", "secondaryForeground", "muted", "mutedForeground",
            "accent", "accentForeground", "destructive", "destructiveForeground", "border", "input", "ring",
            "claude", "codex", "tag", "tagForeground", "visualBorder", "visualWarm", "visualCool", "visualRed", "visualGreen"
        ],
        "metrics": {
            "radius": "integer 2..16",
            "fontScale": "number 0.90..1.15",
            "lineHeight": "number 1.30..1.90",
            "shadow": { "color": "#RRGGBB", "opacity": "0..0.6", "y": "integer 0..16", "blur": "integer 0..40" }
        },
        "atmosphere": { "tint": "#RRGGBB", "noise": "0..0.12", "vignette": "0..0.30" },
        "constraints": [
            "all foreground/background semantic pairs must reach WCAG AA 4.5:1",
            "no CSS, selectors, scripts, URLs, paths, Base64, fonts, animations, or unknown fields",
            "source must be { kind: local }"
        ],
        "example": {
            "schemaVersion": 1,
            "id": "quiet-forest",
            "name": "Quiet Forest",
            "author": "Local Monet user",
            "description": "A calm, high-contrast paper theme.",
            "version": "1.0.0",
            "appearance": "light",
            "source": { "kind": "local" },
            "colors": {
                "background": "#F4F0E8", "foreground": "#24211D",
                "card": "#FFFDF7", "cardForeground": "#24211D",
                "popover": "#FFFDF7", "popoverForeground": "#24211D",
                "primary": "#315A3C", "primaryForeground": "#FFFFFF",
                "secondary": "#E5DED2", "secondaryForeground": "#24211D",
                "muted": "#E9E3D8", "mutedForeground": "#554F47",
                "accent": "#8B302A", "accentForeground": "#FFFFFF",
                "destructive": "#A32626", "destructiveForeground": "#FFFFFF",
                "border": "#BEB6A8", "input": "#CFC7B9", "ring": "#315A3C",
                "claude": "#9B4E2D", "codex": "#405C8A",
                "tag": "#DDE3EE", "tagForeground": "#34445F",
                "visualBorder": "#BEB6A8", "visualWarm": "#F2E3D7",
                "visualCool": "#E5EAF2", "visualRed": "#F2DDDA", "visualGreen": "#DFEADF"
            },
            "metrics": {
                "radius": 6, "fontScale": 1.0, "lineHeight": 1.6,
                "shadow": { "color": "#3C2D1F", "opacity": 0.16, "y": 4, "blur": 12 }
            },
            "atmosphere": { "tint": "#3C2D1F", "noise": 0.02, "vignette": 0.08 }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_theme() -> ThemeDefinition {
        serde_json::from_value(schema_context()["example"].clone())
            .expect("schema example must deserialize")
    }

    #[test]
    fn schema_example_is_a_valid_local_theme() {
        let report = validate_theme(&example_theme());
        assert!(report.valid, "{:?}", report.issues);
    }

    #[test]
    fn unknown_fields_are_rejected_before_preview() {
        let mut raw = schema_context()["example"].clone();
        raw.as_object_mut().expect("example object").insert(
            "css".to_string(),
            serde_json::json!("body { display: none }"),
        );
        assert!(serde_json::from_value::<ThemeDefinition>(raw).is_err());
    }

    #[test]
    fn unsafe_metadata_and_low_contrast_are_reported() {
        let mut theme = example_theme();
        theme.description = "Load https://example.invalid/theme.css".to_string();
        theme.colors.foreground = "#EEEEEE".to_string();
        theme.colors.background = "#FFFFFF".to_string();

        let report = validate_theme(&theme);
        assert!(!report.valid);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.field == "description" && issue.kind == "unsafe"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.field == "foreground/background" && issue.kind == "contrast"));
    }

    #[test]
    fn local_theme_cannot_claim_a_community_issue() {
        let mut theme = example_theme();
        theme.source.issue = Some(42);
        let report = validate_theme(&theme);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.field == "source" && issue.kind == "schema"));
    }
}
