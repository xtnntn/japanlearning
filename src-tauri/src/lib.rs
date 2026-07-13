use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate};
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::HashSet,
    fs,
    path::PathBuf,
    process::Command,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri::{Manager, State};
use tauri_plugin_notification::NotificationExt;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Network(#[from] reqwest::Error),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct AppState {
    db_path: Mutex<PathBuf>,
}

static LOCAL_AI_SETTINGS: OnceLock<Mutex<LocalAiSettings>> = OnceLock::new();
static API_HTTP_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static CHAT_JSON_SCHEMA_CAPABILITY: OnceLock<Mutex<Option<bool>>> = OnceLock::new();

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalAiSettings {
    base_url: Option<String>,
    model: Option<String>,
    protocol: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Article {
    id: String,
    title: String,
    source: String,
    url: String,
    published_at: String,
    paragraphs: Vec<String>,
    images: Vec<String>,
    #[serde(default)]
    embeds: Vec<MediaEmbed>,
    reading_minutes: u32,
    difficulty: String,
    is_exploration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaEmbed {
    kind: String,
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TitleCandidate {
    id: String,
    title: String,
    url: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssessmentQuestion {
    id: String,
    prompt: String,
    choices: Vec<String>,
    answer_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssessmentResult {
    score: usize,
    total: usize,
    level_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyAssessment {
    id: String,
    week: String,
    article: Article,
    questions: Vec<Question>,
    completed: bool,
    result: Option<AssessmentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Explanation {
    reading: String,
    translation: String,
    context_note: String,
    example: String,
    example_translation: String,
    grammar_note: String,
    #[serde(default, deserialize_with = "deserialize_review_candidates")]
    review_candidates: Vec<ReviewCardDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewCardDraft {
    front: String,
    reading: String,
    translation: String,
    context_note: String,
}

fn deserialize_review_candidates<'de, D>(deserializer: D) -> Result<Vec<ReviewCardDraft>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = Vec::<serde_json::Value>::deserialize(deserializer).unwrap_or_default();
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<ReviewCardDraft>(value).ok())
        .take(2)
        .collect())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiStatus {
    configured: bool,
    model: String,
    base_url: String,
    protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Question {
    #[serde(default)]
    id: String,
    prompt: String,
    choices: Vec<String>,
    answer_index: usize,
    evidence: String,
    explanation: String,
    #[serde(default)]
    tested_expressions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct QuestionSet {
    questions: Vec<Question>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    selected_count: i64,
    chinese_reveals: i64,
    completed_articles: i64,
    missed_articles: i64,
    title_votes: i64,
    baseline_completed: bool,
    topic_feedback: Vec<TopicFeedback>,
    selection_trend: Vec<SelectionTrendPoint>,
    assessment_trend: Vec<AssessmentTrendPoint>,
    independent_expression_rate: Option<f64>,
    independent_expression_attempts: i64,
    experiment: ExperimentStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExperimentStatus {
    observed_days: i64,
    completed_days: i64,
    selection_rate_change: Option<f64>,
    weekly_score_non_declining: Option<bool>,
    expression_rate_change: Option<f64>,
    ready_for_verdict: bool,
    verdict: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionTrendPoint {
    day: String,
    normalized_rate: f64,
    selections: i64,
    character_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssessmentTrendPoint {
    week: String,
    score_rate: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReminderStatus {
    enabled: bool,
    hour: u8,
    minute: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AbilityProfile {
    suggested_level: String,
    target_level: Option<String>,
    initial_score: Option<f64>,
    daily_accuracy: Option<f64>,
    weekly_accuracy: Option<f64>,
    selection_count: i64,
    chinese_reveal_rate: Option<f64>,
    completed_articles: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultiAgentPlan {
    target_difficulty: String,
    focus_terms: Vec<String>,
    avoid_terms: Vec<String>,
    article_brief: String,
    question_brief: String,
    rationale: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultiAgentPlanDraft {
    target_difficulty: String,
    focus_terms: Vec<String>,
    avoid_terms: Vec<String>,
    article_brief: String,
    question_brief: String,
    rationale: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewCard {
    id: String,
    front: String,
    reading: String,
    translation: String,
    context_note: String,
    article_title: String,
    review_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TopicFeedback {
    label: String,
    count: i64,
}

fn open_db(state: &AppState) -> Result<Connection, AppError> {
    let path = state
        .db_path
        .lock()
        .map_err(|_| AppError::Message("无法访问本地学习数据库".into()))?
        .clone();
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "
      PRAGMA busy_timeout=5000;
      PRAGMA journal_mode=WAL;
      CREATE TABLE IF NOT EXISTS articles (
        id TEXT PRIMARY KEY,
        day TEXT NOT NULL,
        title TEXT NOT NULL,
        source TEXT NOT NULL,
        url TEXT NOT NULL,
        published_at TEXT NOT NULL,
        paragraphs_json TEXT NOT NULL,
        images_json TEXT NOT NULL DEFAULT '[]',
        embeds_json TEXT NOT NULL DEFAULT '[]',
        reading_minutes INTEGER NOT NULL,
        difficulty TEXT NOT NULL,
        is_exploration INTEGER NOT NULL DEFAULT 0,
        completed_at TEXT
      );
      CREATE TABLE IF NOT EXISTS selections (
        id TEXT PRIMARY KEY,
        article_id TEXT NOT NULL,
        selection TEXT NOT NULL,
        context TEXT NOT NULL,
        chinese_revealed INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS review_cards (
        id TEXT PRIMARY KEY,
        selection_id TEXT NOT NULL UNIQUE,
        article_id TEXT NOT NULL,
        front TEXT NOT NULL,
        reading TEXT NOT NULL,
        translation TEXT NOT NULL,
        context_note TEXT NOT NULL,
        review_count INTEGER NOT NULL DEFAULT 0,
        interval_days INTEGER NOT NULL DEFAULT 1,
        due_at TEXT NOT NULL,
        last_reviewed_at TEXT,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS topic_feedback (
        id TEXT PRIMARY KEY,
        article_id TEXT NOT NULL,
        label TEXT NOT NULL,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS answers (
        id TEXT PRIMARY KEY,
        article_id TEXT NOT NULL,
        question_id TEXT NOT NULL,
        chosen_index INTEGER NOT NULL,
        correct INTEGER NOT NULL,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS title_votes (
        id TEXT PRIMARY KEY,
        candidate_id TEXT NOT NULL,
        title TEXT NOT NULL,
        url TEXT NOT NULL,
        vote TEXT NOT NULL,
        created_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS assessments (
        id TEXT PRIMARY KEY,
        mode TEXT NOT NULL,
        score INTEGER NOT NULL,
        total INTEGER NOT NULL,
        completed_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS weekly_assessments (
        id TEXT PRIMARY KEY,
        week TEXT NOT NULL UNIQUE,
        article_json TEXT NOT NULL,
        questions_json TEXT NOT NULL,
        score INTEGER,
        total INTEGER,
        completed_at TEXT
      );
      CREATE TABLE IF NOT EXISTS app_settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS expression_evidence (
        id TEXT PRIMARY KEY,
        article_id TEXT NOT NULL,
        question_id TEXT NOT NULL,
        expression TEXT NOT NULL,
        correct INTEGER NOT NULL,
        used_assistance INTEGER NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(article_id, question_id, expression)
      );
      CREATE TABLE IF NOT EXISTS article_questions (
        article_id TEXT PRIMARY KEY,
        questions_json TEXT NOT NULL,
        generated_at TEXT NOT NULL
      );
    ",
    )?;
    let image_column_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = 'images_json'",
        [],
        |row| row.get(0),
    )?;
    if image_column_exists == 0 {
        conn.execute(
            "ALTER TABLE articles ADD COLUMN images_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    let embeds_column_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = 'embeds_json'",
        [],
        |row| row.get(0),
    )?;
    if embeds_column_exists == 0 {
        conn.execute(
            "ALTER TABLE articles ADD COLUMN embeds_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    Ok(conn)
}

fn reminder_plist_path() -> Result<PathBuf, AppError> {
    let home =
        std::env::var_os("HOME").ok_or_else(|| AppError::Message("无法定位用户目录".into()))?;
    Ok(PathBuf::from(home)
        .join("Library/LaunchAgents/com.xtnntn.nihongo-daily-reader.reminder.plist"))
}

fn write_daily_reminder_plist(hour: u8, minute: u8) -> Result<(), AppError> {
    let executable = std::env::current_exe()
        .map_err(|error| AppError::Message(format!("无法定位应用程序：{error}")))?;
    let plist_path = reminder_plist_path()?;
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::Message(format!("无法创建提醒目录：{error}")))?;
    }
    let escape = |value: &str| {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.xtnntn.nihongo-daily-reader.reminder</string>
<key>ProgramArguments</key><array><string>{}</string><string>--daily-reminder</string></array>
<key>StartCalendarInterval</key><dict><key>Hour</key><integer>{}</integer><key>Minute</key><integer>{}</integer></dict>
<key>ProcessType</key><string>Background</string>
</dict></plist>"#,
        escape(&executable.to_string_lossy()),
        hour,
        minute
    );
    fs::write(&plist_path, plist)
        .map_err(|error| AppError::Message(format!("无法保存提醒配置：{error}")))?;
    let _ = Command::new("launchctl")
        .args(["unload", &plist_path.to_string_lossy()])
        .status();
    let status = Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .status()
        .map_err(|error| AppError::Message(format!("无法启用提醒：{error}")))?;
    if !status.success() {
        return Err(AppError::Message("launchd 未能启用每日提醒".into()));
    }
    Ok(())
}

#[tauri::command]
fn get_reminder_status(state: State<'_, AppState>) -> Result<ReminderStatus, AppError> {
    let conn = open_db(&state)?;
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'daily_reminder'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let (enabled, hour, minute) = value
        .and_then(|value| {
            let mut parts = value.split(':');
            Some((
                true,
                parts.next()?.parse().ok()?,
                parts.next()?.parse().ok()?,
            ))
        })
        .unwrap_or((false, 9, 0));
    // Product renames change the .app executable path. Refresh an already authorized
    // reminder when the main window opens so the next schedule uses the new bundle.
    if enabled {
        write_daily_reminder_plist(hour, minute)?;
    }
    Ok(ReminderStatus {
        enabled,
        hour,
        minute,
    })
}

#[tauri::command]
fn install_daily_reminder(
    state: State<'_, AppState>,
    hour: u8,
    minute: u8,
) -> Result<ReminderStatus, AppError> {
    if hour > 23 || minute > 59 {
        return Err(AppError::Message("提醒时间无效".into()));
    }
    write_daily_reminder_plist(hour, minute)?;
    let conn = open_db(&state)?;
    conn.execute("INSERT INTO app_settings (key, value) VALUES ('daily_reminder', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![format!("{hour:02}:{minute:02}")])?;
    Ok(ReminderStatus {
        enabled: true,
        hour,
        minute,
    })
}

#[tauri::command]
fn remove_daily_reminder(state: State<'_, AppState>) -> Result<ReminderStatus, AppError> {
    let plist_path = reminder_plist_path()?;
    if plist_path.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .status();
        fs::remove_file(&plist_path)
            .map_err(|error| AppError::Message(format!("无法移除提醒配置：{error}")))?;
    }
    open_db(&state)?.execute("DELETE FROM app_settings WHERE key = 'daily_reminder'", [])?;
    Ok(ReminderStatus {
        enabled: false,
        hour: 9,
        minute: 0,
    })
}

fn local_ai_settings_path() -> Result<PathBuf, AppError> {
    let home =
        std::env::var_os("HOME").ok_or_else(|| AppError::Message("无法定位用户目录".into()))?;
    Ok(PathBuf::from(home)
        .join("Library/Application Support/com.xtnntn.nihongo-daily-reader/ai-settings.json"))
}

fn load_local_ai_settings() -> LocalAiSettings {
    local_ai_settings_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn local_ai_settings() -> LocalAiSettings {
    LOCAL_AI_SETTINGS
        .get_or_init(|| Mutex::new(load_local_ai_settings()))
        .lock()
        .map(|settings| settings.clone())
        .unwrap_or_default()
}

fn save_local_ai_settings(settings: LocalAiSettings) -> Result<(), AppError> {
    let path = local_ai_settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::Message(format!("无法保存 AI 设置：{error}")))?;
    }
    fs::write(
        &path,
        serde_json::to_string(&settings)
            .map_err(|error| AppError::Message(format!("无法编码 AI 设置：{error}")))?,
    )
    .map_err(|error| AppError::Message(format!("无法保存 AI 设置：{error}")))?;
    #[cfg(unix)]
    fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|error| AppError::Message(format!("无法收紧 AI 设置文件权限：{error}")))?;
    if let Ok(mut cached) = LOCAL_AI_SETTINGS
        .get_or_init(|| Mutex::new(LocalAiSettings::default()))
        .lock()
    {
        *cached = settings;
    }
    Ok(())
}

fn get_api_key() -> Option<String> {
    local_ai_settings().api_key.filter(|key| !key.trim().is_empty())
}

fn get_base_url() -> String {
    local_ai_settings()
        .base_url
        .unwrap_or_else(|| "https://api.openai.com/v1".into())
}

fn get_model() -> String {
    local_ai_settings()
        .model
        .unwrap_or_else(|| "gpt-5.6-luna".into())
}

fn get_protocol() -> String {
    local_ai_settings()
        .protocol
        .filter(|protocol| matches!(protocol.as_str(), "responses" | "chat_completions"))
        .unwrap_or_else(|| "responses".into())
}

fn ai_is_configured_without_keychain_access() -> bool {
    let settings = local_ai_settings();
    settings.api_key.is_some() && settings.base_url.is_some() && settings.model.is_some() && settings.protocol.is_some()
}

fn responses_url(base_url: &str) -> Result<String, AppError> {
    let mut base = base_url.trim().trim_end_matches('/').to_string();
    let parsed = url::Url::parse(&base)
        .map_err(|_| AppError::Message("Base URL 必须是完整的 https:// 地址".into()))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Message(
            "为保护 API Key 和学习内容，Base URL 必须使用 https://".into(),
        ));
    }
    if !base.ends_with("/responses") {
        base.push_str("/responses");
    }
    Ok(base)
}

fn chat_completions_url(base_url: &str) -> Result<String, AppError> {
    let mut base = base_url.trim().trim_end_matches('/').to_string();
    let parsed = url::Url::parse(&base)
        .map_err(|_| AppError::Message("Base URL 必须是完整的 https:// 地址".into()))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Message(
            "为保护 API Key 和学习内容，Base URL 必须使用 https://".into(),
        ));
    }
    if !base.ends_with("/chat/completions") {
        base.push_str("/chat/completions");
    }
    Ok(base)
}

fn models_url(base_url: &str) -> Result<String, AppError> {
    let responses_endpoint = responses_url(base_url)?;
    Ok(responses_endpoint
        .trim_end_matches("/responses")
        .to_string()
        + "/models")
}

fn api_http_client() -> Result<&'static Client, AppError> {
    API_HTTP_CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(30))
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| AppError::Message(format!("无法初始化 AI 网络连接：{error}")))
}

fn cached_json_schema_capability() -> Option<bool> {
    CHAT_JSON_SCHEMA_CAPABILITY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|value| *value)
}

fn remember_json_schema_capability(value: bool) {
    if let Ok(mut capability) = CHAT_JSON_SCHEMA_CAPABILITY
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *capability = Some(value);
    }
}

fn chat_content_text(content: &serde_json::Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    content.as_array().map(|parts| {
        parts.iter().filter_map(|part| {
            part.get("text").and_then(|value| value.as_str())
                .or_else(|| part.get("content").and_then(|value| value.as_str()))
        }).collect::<Vec<_>>().join("")
    }).filter(|text| !text.is_empty())
}

fn responses_content_text(body: &serde_json::Value) -> Option<String> {
    body.get("output_text").and_then(|value| value.as_str()).map(str::to_owned).or_else(|| {
        body.get("output").and_then(|value| value.as_array()).map(|items| {
            items.iter().flat_map(|item| item.get("content").and_then(|value| value.as_array()).into_iter().flatten())
                .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
                .collect::<Vec<_>>().join("")
        }).filter(|text| !text.is_empty())
    })
}

fn parse_json_value(output: &str) -> Result<serde_json::Value, String> {
    let cleaned = output.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim();
    if let Ok(value) = serde_json::from_str(cleaned) {
        return Ok(value);
    }
    let start = cleaned.find('{').ok_or_else(|| "没有找到 JSON 对象起点".to_string())?;
    let end = cleaned.rfind('}').ok_or_else(|| "没有找到 JSON 对象终点".to_string())?;
    serde_json::from_str(&cleaned[start..=end]).map_err(|error| error.to_string())
}

async fn generate_structured<T: for<'de> Deserialize<'de>>(
    api_key: &str,
    system: &str,
    user: &str,
    schema_name: &str,
    schema: serde_json::Value,
) -> Result<T, AppError> {
    let client = api_http_client()?;
    let protocol = get_protocol();
    let mut last_error = String::new();
    for attempt in 0..3 {
        let output = if protocol == "chat_completions" {
            let endpoint = chat_completions_url(&get_base_url())?;
            let chat_system = format!(
                "{system}\n\n必须只返回一个符合要求的 JSON 对象，不要 Markdown 或额外说明。"
            );
            let mut payload = serde_json::json!({
              "model": get_model(),
              "max_tokens": match schema_name { "selection_explanation" => 900, "reading_questions" => 2400, _ => 1200 },
              "messages": [
                { "role": "system", "content": chat_system },
                { "role": "user", "content": user }
              ],
            });
            // A successful HTTP response can still contain an empty `content` on some
            // OpenAI-compatible routers. The second attempt deliberately avoids strict
            // schema mode instead of repeating the exact same request.
            let use_json_schema = attempt == 0 && cached_json_schema_capability().unwrap_or(true);
            if attempt < 2 {
                payload["response_format"] = if use_json_schema {
                    serde_json::json!({ "type": "json_schema", "json_schema": { "name": schema_name, "strict": true, "schema": schema.clone() } })
                } else {
                    serde_json::json!({ "type": "json_object" })
                };
            }
            let mut response = client
                .post(&endpoint)
                .bearer_auth(api_key)
                .json(&payload)
                .send()
                .await?;
            if use_json_schema
                && !response.status().is_success()
                && matches!(response.status().as_u16(), 400 | 422)
            {
                remember_json_schema_capability(false);
                payload["response_format"] = serde_json::json!({ "type": "json_object" });
                response = client
                    .post(&endpoint)
                    .bearer_auth(api_key)
                    .json(&payload)
                    .send()
                    .await?;
            }
            if !response.status().is_success() && matches!(response.status().as_u16(), 400 | 422) {
                payload
                    .as_object_mut()
                    .map(|object| object.remove("response_format"));
                response = client
                    .post(&endpoint)
                    .bearer_auth(api_key)
                    .json(&payload)
                    .send()
                    .await?;
            }
            if !response.status().is_success() {
                let status = response.status();
                let detail = response.text().await.unwrap_or_default();
                return Err(AppError::Message(format!(
                    "Chat Completions 请求失败：{status} {}",
                    detail.chars().take(280).collect::<String>()
                )));
            }
            let body: serde_json::Value = response.json().await?;
            body.get("choices")
                .and_then(|choices| choices.as_array())
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("message"))
                .and_then(|message| {
                    message.get("content").and_then(chat_content_text)
                        .or_else(|| message.get("reasoning_content").and_then(chat_content_text))
                })
                .ok_or_else(|| {
                    AppError::Message("Chat Completions 未返回 choices[0].message.content".into())
                })?
        } else {
            let endpoint = responses_url(&get_base_url())?;
            let payload = serde_json::json!({
            "model": get_model(),
            "reasoning": { "effort": "low" },
            "input": [
              { "role": "system", "content": [{ "type": "input_text", "text": system }] },
              { "role": "user", "content": [{ "type": "input_text", "text": user }] }
            ],
            "text": {
              "format": {
                "type": "json_schema",
                "name": schema_name,
                "strict": true,
                "schema": schema.clone()
              }
            }
            });
            let response = client
                .post(endpoint)
                .bearer_auth(api_key)
                .json(&payload)
                .send()
                .await?;
            if !response.status().is_success() {
                let status = response.status();
                let detail = response.text().await.unwrap_or_default();
                return Err(AppError::Message(format!(
                    "Responses 请求失败：{status} {}",
                    detail.chars().take(280).collect::<String>()
                )));
            }
            let body: serde_json::Value = response.json().await?;
            responses_content_text(&body)
                .ok_or_else(|| AppError::Message("Responses 未返回 output_text".into()))?
        };
        let output = output.trim();
        if output.is_empty() {
            remember_json_schema_capability(false);
            last_error = "AI 返回空内容".into();
            continue;
        }
        match parse_json_value(output).and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string())) {
            Ok(value) => {
                if protocol == "chat_completions" { remember_json_schema_capability(true); }
                return Ok(value);
            }
            Err(error) => {
                remember_json_schema_capability(false);
                last_error = error;
            }
        }
    }
    Err(AppError::Message(format!("AI 返回格式异常，已自动兼容并重试：{last_error}")))
}

fn explanation_schema() -> serde_json::Value {
    serde_json::json!({
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "reading": { "type": "string" },
        "translation": { "type": "string" },
        "contextNote": { "type": "string" },
        "example": { "type": "string" },
        "exampleTranslation": { "type": "string" },
        "grammarNote": { "type": "string" }
        ,"reviewCandidates": { "type": "array", "maxItems": 2, "items": { "type": "object", "additionalProperties": false, "properties": { "front": { "type": "string" }, "reading": { "type": "string" }, "translation": { "type": "string" }, "contextNote": { "type": "string" } }, "required": ["front", "reading", "translation", "contextNote"] } }
      },
      "required": ["reading", "translation", "contextNote", "example", "exampleTranslation", "grammarNote", "reviewCandidates"]
    })
}

fn question_schema() -> serde_json::Value {
    serde_json::json!({
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "questions": {
          "type": "array",
          "minItems": 3,
          "maxItems": 3,
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": { "type": "string" },
              "prompt": { "type": "string" },
              "choices": { "type": "array", "minItems": 4, "maxItems": 4, "items": { "type": "string" } },
              "answerIndex": { "type": "integer", "minimum": 0, "maximum": 3 },
              "evidence": { "type": "string" },
              "explanation": { "type": "string" }
              ,"testedExpressions": { "type": "array", "maxItems": 3, "items": { "type": "string" } }
            },
            "required": ["prompt", "choices", "answerIndex", "evidence", "explanation", "testedExpressions"]
          }
        }
      },
      "required": ["questions"]
    })
}

fn ensure_question_ids(questions: &mut [Question]) {
    for (index, question) in questions.iter_mut().enumerate() {
        if question.id.trim().is_empty() {
            question.id = format!("ai-question-{}-{}", index + 1, Uuid::new_v4());
        }
    }
}

fn multi_agent_plan_schema() -> serde_json::Value {
    serde_json::json!({
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "targetDifficulty": { "type": "string" },
        "focusTerms": { "type": "array", "maxItems": 6, "items": { "type": "string" } },
        "avoidTerms": { "type": "array", "maxItems": 4, "items": { "type": "string" } },
        "articleBrief": { "type": "string" },
        "questionBrief": { "type": "string" },
        "rationale": { "type": "string" }
      },
      "required": ["targetDifficulty", "focusTerms", "avoidTerms", "articleBrief", "questionBrief", "rationale"]
    })
}

fn initial_assessment() -> Vec<AssessmentQuestion> {
    vec![
    AssessmentQuestion { id: "v1".into(), prompt: "「発売」の読み方として正しいものはどれですか。".into(), choices: vec!["はつばい".into(), "はっばい".into(), "はつうり".into(), "はつまい".into()], answer_index: 0 },
    AssessmentQuestion { id: "v2".into(), prompt: "「配信を楽しみにしています」の意味に最も近いものはどれですか。".into(), choices: vec!["我正在期待直播。".into(), "我不想看直播。".into(), "我已经结束直播。".into(), "我在制作直播。".into()], answer_index: 0 },
    AssessmentQuestion { id: "g1".into(), prompt: "次の文の（　）に入る最も自然なものはどれですか。\n新しい情報は、あとで確認する（　）メモしておこう。".into(), choices: vec!["ために".into(), "ながら".into(), "ので".into(), "しか".into()], answer_index: 0 },
    AssessmentQuestion { id: "g2".into(), prompt: "「まだ見ていない」の意味として正しいものはどれですか。".into(), choices: vec!["还没有看。".into(), "已经看完。".into(), "绝对不看。".into(), "正在看。".into()], answer_index: 0 },
    AssessmentQuestion { id: "g3".into(), prompt: "次の文とほぼ同じ意味の文を選んでください。\nこの記事は、初心者でも読めるように書かれている。".into(), choices: vec!["初めて読む人にも分かるように書かれている。".into(), "専門家だけのために書かれている。".into(), "読むことが禁止されている。".into(), "内容が全くない。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r1".into(), prompt: "次の文章を読んでください。\n「イベントは予定より一時間遅れて始まった。しかし、出演者が登場すると、会場はすぐに盛り上がった。」\nイベントについて正しいものはどれですか。".into(), choices: vec!["開始は予定より遅かった。".into(), "出演者は来なかった。".into(), "会場は最初から静かだった。".into(), "イベントは中止になった。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r2".into(), prompt: "同じ文章について、「しかし」が表す関係として最も近いものはどれですか。".into(), choices: vec!["前の内容とは違う展開".into(), "同じ内容の繰り返し".into(), "理由の説明".into(), "時間の順番".into()], answer_index: 0 },
    AssessmentQuestion { id: "r3".into(), prompt: "次の文章を読んでください。\n「公式サイトで発表された内容によると、新作ゲームの発売日は来月に変更された。開発チームは品質を上げるために、もう少し時間が必要だと説明している。」\n発売日が変更された主な理由は何ですか。".into(), choices: vec!["品質を上げるため".into(), "公式サイトが閉じたため".into(), "ゲームが完成したため".into(), "来月が休みのため".into()], answer_index: 0 },
    AssessmentQuestion { id: "r4".into(), prompt: "同じ文章で「によると」はどのような働きですか。".into(), choices: vec!["情報の出所を示す".into(), "命令を表す".into(), "比較を表す".into(), "質問を表す".into()], answer_index: 0 },
    AssessmentQuestion { id: "r5".into(), prompt: "「〜わけではない」を使った文として正しい意味を選んでください。\n日本のアニメなら、何でも好きなわけではない。".into(), choices: vec!["所有日本动画我都喜欢，并不是这个意思。".into(), "我完全不喜欢日本动画。".into(), "我只看日本动画。".into(), "日本动画不存在。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r6".into(), prompt: "次の文で、筆者の意見を表している部分はどれですか。\n「この作品は話題になっているが、私は物語より音楽のほうが印象に残った。」".into(), choices: vec!["私は物語より音楽のほうが印象に残った。".into(), "この作品は話題になっている。".into(), "作品という言葉。".into(), "なっているという表現。".into()], answer_index: 0 },
    AssessmentQuestion { id: "r7".into(), prompt: "次の文章の要点として最も近いものを選んでください。\n「ニュースを読むとき、知らない単語だけに注目すると、記事全体の意味を見失いやすい。まず見出しと段落の流れをつかみ、必要な表現だけを確認するほうが理解しやすい。」".into(), choices: vec!["先に全体の流れをつかむことが大切だ。".into(), "すべての単語を辞書で調べるべきだ。".into(), "見出しは読まなくてよい。".into(), "ニュースは読まないほうがよい。".into()], answer_index: 0 },
  ]
}

fn article_from_row(row: &rusqlite::Row<'_>) -> Result<Article, rusqlite::Error> {
    let paragraphs_json: String = row.get("paragraphs_json")?;
    let images_json: String = row.get("images_json")?;
    let embeds_json: String = row.get("embeds_json")?;
    Ok(Article {
        id: row.get("id")?,
        title: row.get("title")?,
        source: row.get("source")?,
        url: row.get("url")?,
        published_at: row.get("published_at")?,
        paragraphs: serde_json::from_str(&paragraphs_json).unwrap_or_default(),
        images: serde_json::from_str(&images_json).unwrap_or_default(),
        embeds: serde_json::from_str(&embeds_json).unwrap_or_default(),
        reading_minutes: row.get("reading_minutes")?,
        difficulty: row.get("difficulty")?,
        is_exploration: row.get::<_, i64>("is_exploration")? == 1,
    })
}

fn save_article(conn: &Connection, article: &Article) -> Result<(), AppError> {
    conn.execute(
    "INSERT OR REPLACE INTO articles
      (id, day, title, source, url, published_at, paragraphs_json, images_json, embeds_json, reading_minutes, difficulty, is_exploration)
      VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    params![
      article.id,
      Local::now().format("%Y-%m-%d").to_string(),
      article.title,
      article.source,
      article.url,
      article.published_at,
      serde_json::to_string(&article.paragraphs)
        .map_err(|e| AppError::Message(format!("文章保存失败：{e}")))?,
      serde_json::to_string(&article.images)
        .map_err(|e| AppError::Message(format!("图片保存失败：{e}")))?,
      serde_json::to_string(&article.embeds)
        .map_err(|e| AppError::Message(format!("嵌入内容保存失败：{e}")))?,
      article.reading_minutes,
      article.difficulty,
      article.is_exploration as i64,
    ],
  )?;
    Ok(())
}

fn is_sample_article(article: &Article) -> bool {
    article.id == "sample-kaiyou-reader" || article.source.contains("接続確認用サンプル")
}

fn remove_article_and_learning_artifacts(
    conn: &Connection,
    article_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM review_cards WHERE article_id = ?1",
        params![article_id],
    )?;
    conn.execute(
        "DELETE FROM selections WHERE article_id = ?1",
        params![article_id],
    )?;
    conn.execute(
        "DELETE FROM topic_feedback WHERE article_id = ?1",
        params![article_id],
    )?;
    conn.execute(
        "DELETE FROM answers WHERE article_id = ?1",
        params![article_id],
    )?;
    conn.execute("DELETE FROM articles WHERE id = ?1", params![article_id])?;
    Ok(())
}

fn title_bigrams(text: &str) -> HashSet<String> {
    let chars = text
        .chars()
        .filter(|ch| !ch.is_whitespace() && !ch.is_ascii_punctuation())
        .collect::<Vec<_>>();
    chars.windows(2).map(|pair| pair.iter().collect()).collect()
}

fn overlap_score(title: &str, examples: &[String]) -> f64 {
    let grams = title_bigrams(title);
    examples
        .iter()
        .map(|example| {
            let example_grams = title_bigrams(example);
            grams.intersection(&example_grams).count() as f64
        })
        .sum()
}

fn personalized_candidates(
    conn: &Connection,
    mut candidates: Vec<TitleCandidate>,
    exploration: bool,
    agent_plan: Option<&MultiAgentPlan>,
) -> Result<Vec<TitleCandidate>, AppError> {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let mut statement = conn.prepare("SELECT title, vote FROM title_votes")?;
    for item in statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(Result::ok)
    {
        if item.1 == "想看" {
            positive.push(item.0);
        } else if item.1 == "不想看" {
            negative.push(item.0);
        }
    }
    if let Some(plan) = agent_plan {
        positive.extend(plan.focus_terms.iter().cloned());
        negative.extend(plan.avoid_terms.iter().cloned());
    }
    let mut statement = conn.prepare(
        "SELECT a.title, f.label FROM topic_feedback f JOIN articles a ON a.id = f.article_id",
    )?;
    for item in statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(Result::ok)
    {
        if item.1 == "想多读这个题材" {
            positive.push(item.0);
        } else if item.1 == "题材不感兴趣" {
            negative.push(item.0);
        }
    }
    candidates.sort_by(|left, right| {
        let score = |candidate: &TitleCandidate| {
            overlap_score(&candidate.title, &positive)
                - overlap_score(&candidate.title, &negative) * 1.4
        };
        let left_score = score(left);
        let right_score = score(right);
        if exploration {
            left_score.total_cmp(&right_score)
        } else {
            right_score.total_cmp(&left_score)
        }
    });
    Ok(candidates)
}

fn automatic_difficulty(conn: &Connection) -> Result<String, AppError> {
    let initial: Option<(i64, i64)> = conn.query_row(
    "SELECT score, total FROM assessments WHERE mode = 'initial' ORDER BY completed_at DESC LIMIT 1",
    [], |row| Ok((row.get(0)?, row.get(1)?))
  ).optional()?;
    let base = initial
        .map(|(score, total)| {
            if total > 0 {
                score as f64 / total as f64
            } else {
                0.5
            }
        })
        .unwrap_or(0.5);
    let recent_density: Option<f64> = conn.query_row(
    "SELECT AVG(selection_count * 1000.0 / character_count) FROM (
       SELECT a.id, MAX(LENGTH(a.paragraphs_json), 1) AS character_count, COUNT(s.id) AS selection_count
       FROM articles a LEFT JOIN selections s ON s.article_id = a.id
       WHERE a.completed_at IS NOT NULL GROUP BY a.id ORDER BY a.day DESC LIMIT 3
     )", [], |row| row.get(0)
  ).optional()?.flatten();
    let difficulty = match (base, recent_density.unwrap_or(0.0)) {
        (_, density) if density > 10.0 => "N4–N3",
        (score, _) if score >= 0.75 => "N2",
        (score, _) if score >= 0.45 => "N3–N2",
        _ => "N4–N3",
    };
    Ok(difficulty.into())
}

fn multi_agent_plan(conn: &Connection) -> Option<MultiAgentPlan> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = 'multi_agent_plan'",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .and_then(|value| serde_json::from_str(&value).ok())
}

fn effective_difficulty(
    conn: &Connection,
    plan: Option<&MultiAgentPlan>,
) -> Result<String, AppError> {
    let manual_level: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'target_level'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(level) =
        manual_level.filter(|level| ["N5", "N4", "N3", "N2", "N1"].contains(&level.as_str()))
    {
        return Ok(level);
    }
    if let Some(level) = plan
        .map(|plan| plan.target_difficulty.as_str())
        .filter(|level| ["N5", "N4", "N3", "N2", "N1", "N4–N3", "N3–N2"].contains(level))
    {
        return Ok(level.into());
    }
    automatic_difficulty(conn)
}

fn learning_evidence(conn: &Connection) -> Result<String, AppError> {
    let ability = get_ability_profile_from_conn(conn)?;
    let recent_selections = conn.prepare(
    "SELECT selection, COUNT(*) AS count FROM selections GROUP BY selection ORDER BY count DESC, MAX(created_at) DESC LIMIT 12"
  )?.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
    .filter_map(Result::ok).map(|(selection, count)| format!("{selection} ×{count}")).collect::<Vec<_>>();
    let feedback = conn
        .prepare(
            "SELECT label, COUNT(*) FROM topic_feedback GROUP BY label ORDER BY COUNT(*) DESC",
        )?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(Result::ok)
        .map(|(label, count)| format!("{label} ×{count}"))
        .collect::<Vec<_>>();
    Ok(format!(
    "系统建议难度：{}\n手动目标：{}\n完成文章：{}\n日常理解正确率：{}\n独立周测正确率：{}\n划词总数：{}\n高频划词：{}\n题材反馈：{}",
    ability.suggested_level,
    ability.target_level.unwrap_or_else(|| "自动".into()),
    ability.completed_articles,
    ability.daily_accuracy.map(|value| format!("{:.0}%", value * 100.0)).unwrap_or_else(|| "暂无".into()),
    ability.weekly_accuracy.map(|value| format!("{:.0}%", value * 100.0)).unwrap_or_else(|| "暂无".into()),
    ability.selection_count,
    if recent_selections.is_empty() { "暂无".into() } else { recent_selections.join("、") },
    if feedback.is_empty() { "暂无".into() } else { feedback.join("、") },
  ))
}

fn optional_ratio(conn: &Connection, sql: &str) -> Result<Option<f64>, AppError> {
    let (numerator, denominator): (i64, i64) =
        conn.query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok((denominator > 0).then_some(numerator as f64 / denominator as f64))
}

fn get_ability_profile_from_conn(conn: &Connection) -> Result<AbilityProfile, AppError> {
    let initial_score = optional_ratio(conn, "SELECT COALESCE(SUM(score), 0), COALESCE(SUM(total), 0) FROM assessments WHERE mode = 'initial'")?;
    let daily_accuracy = optional_ratio(
        conn,
        "SELECT COALESCE(SUM(correct), 0), COUNT(*) FROM answers",
    )?;
    let weekly_accuracy = optional_ratio(conn, "SELECT COALESCE(SUM(score), 0), COALESCE(SUM(total), 0) FROM weekly_assessments WHERE completed_at IS NOT NULL")?;
    let selection_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM selections", [], |row| row.get(0))?;
    let chinese_reveals: i64 = conn.query_row(
        "SELECT COUNT(*) FROM selections WHERE chinese_revealed = 1",
        [],
        |row| row.get(0),
    )?;
    let completed_articles = conn.query_row(
        "SELECT COUNT(*) FROM articles WHERE completed_at IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    let target_level = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'target_level'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(AbilityProfile {
        suggested_level: automatic_difficulty(conn)?,
        target_level,
        initial_score,
        daily_accuracy,
        weekly_accuracy,
        selection_count,
        chinese_reveal_rate: (selection_count > 0)
            .then_some(chinese_reveals as f64 / selection_count as f64),
        completed_articles,
    })
}

#[tauri::command]
fn get_ability_profile(state: State<'_, AppState>) -> Result<AbilityProfile, AppError> {
    let conn = open_db(&state)?;
    get_ability_profile_from_conn(&conn)
}

#[tauri::command]
fn update_target_level(
    state: State<'_, AppState>,
    target_level: Option<String>,
) -> Result<AbilityProfile, AppError> {
    let conn = open_db(&state)?;
    match target_level.as_deref() {
        None | Some("") => {
            conn.execute("DELETE FROM app_settings WHERE key = 'target_level'", [])?;
        }
        Some(level) if ["N5", "N4", "N3", "N2", "N1"].contains(&level) => {
            conn.execute("INSERT INTO app_settings (key, value) VALUES ('target_level', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![level])?;
        }
        _ => {
            return Err(AppError::Message(
                "目标难度必须是 N5 到 N1，或使用自动判断".into(),
            ))
        }
    }
    drop(conn);
    get_ability_profile(state)
}

#[tauri::command]
fn get_multi_agent_plan(state: State<'_, AppState>) -> Result<Option<MultiAgentPlan>, AppError> {
    let conn = open_db(&state)?;
    Ok(multi_agent_plan(&conn))
}

#[tauri::command]
async fn refresh_multi_agent_plan(state: State<'_, AppState>) -> Result<MultiAgentPlan, AppError> {
    let api_key =
        get_api_key().ok_or_else(|| AppError::Message("请先在 AI 工作台配置访问密钥。".into()))?;
    let evidence = {
        let conn = open_db(&state)?;
        learning_evidence(&conn)?
    };
    let system = "你是 Kotoba Atelier 的学习分析师。仅根据提供的本地学习证据，为一位中文母语者制定下一阶段日语阅读策略。targetDifficulty 只能为 N5、N4、N3、N2、N1、N4–N3 或 N3–N2。focusTerms 给 2 到 6 个适合在真实 ACGN/虚拟主播文章中继续遇到的日语表达或主题词；avoidTerms 最多 4 个当前应暂缓的主题/表达。articleBrief 用简洁中文说明内容策展人应如何选文；questionBrief 用简洁中文说明出题教练应重点验证什么；rationale 用简洁中文解释，不得宣称没有证据的掌握程度。";
    let user = format!("学习证据：\n{evidence}");
    let draft: MultiAgentPlanDraft = generate_structured(
        &api_key,
        system,
        &user,
        "learning_analysis",
        multi_agent_plan_schema(),
    )
    .await?;
    let allowed = ["N5", "N4", "N3", "N2", "N1", "N4–N3", "N3–N2"];
    let plan = MultiAgentPlan {
        target_difficulty: if allowed.contains(&draft.target_difficulty.as_str()) {
            draft.target_difficulty
        } else {
            "N4–N3".into()
        },
        focus_terms: draft
            .focus_terms
            .into_iter()
            .map(|term| term.trim().to_string())
            .filter(|term| !term.is_empty())
            .take(6)
            .collect(),
        avoid_terms: draft
            .avoid_terms
            .into_iter()
            .map(|term| term.trim().to_string())
            .filter(|term| !term.is_empty())
            .take(4)
            .collect(),
        article_brief: draft.article_brief.trim().chars().take(160).collect(),
        question_brief: draft.question_brief.trim().chars().take(160).collect(),
        rationale: draft.rationale.trim().chars().take(220).collect(),
        updated_at: Local::now().to_rfc3339(),
    };
    let conn = open_db(&state)?;
    conn.execute(
    "INSERT INTO app_settings (key, value) VALUES ('multi_agent_plan', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    params![serde_json::to_string(&plan).map_err(|error| AppError::Message(format!("学习策略保存失败：{error}")))?],
  )?;
    Ok(plan)
}

async fn fetch_kaiyou_candidates() -> Result<Vec<TitleCandidate>, AppError> {
    let client = Client::builder()
        .user_agent("NihongoDailyReader/0.1 (personal learning reader)")
        .build()?;
    let home = client
        .get("https://kai-you.net/")
        .send()
        .await?
        .text()
        .await?;
    let document = Html::parse_document(&home);
    let link_selector = Selector::parse("a[href^='/article/']")
        .map_err(|_| AppError::Message("无法解析 KAI-YOU 首页".into()))?;
    let mut seen = HashSet::new();
    let mut candidates = document
        .select(&link_selector)
        .filter_map(|link| {
            let title = link.text().collect::<Vec<_>>().join(" ").trim().to_string();
            let href = link.value().attr("href")?.to_string();
            if title.chars().count() < 12 || !seen.insert(href.clone()) {
                return None;
            }
            Some(TitleCandidate {
                id: href.trim_start_matches('/').replace('/', "-"),
                title,
                url: format!("https://kai-you.net{href}"),
                source: "KAI-YOU".into(),
            })
        })
        .collect::<Vec<_>>();
    // The homepage mixes navigation, featured stories and older links in DOM order.
    // Restricting the first dozen links made the daily task fail whenever those few
    // pages were short, restricted, or already read. Article IDs are chronological.
    candidates.sort_by_key(|candidate| {
        Reverse(
            candidate
                .url
                .rsplit('/')
                .next()
                .and_then(|id| id.parse::<u64>().ok())
                .unwrap_or_default(),
        )
    });
    candidates.truncate(80);
    if candidates.is_empty() {
        return Err(AppError::Message("KAI-YOU 首页没有找到可读文章".into()));
    }
    Ok(candidates)
}

async fn fetch_personalized_kaiyou_article(db_path: &PathBuf) -> Result<Article, AppError> {
    let exploration = matches!(
        Local::now().weekday(),
        chrono::Weekday::Tue | chrono::Weekday::Fri
    );
    let fetched_candidates = fetch_kaiyou_candidates().await?;
    let (candidates, difficulty, used_urls) = {
        let conn = Connection::open(db_path)?;
        let plan = multi_agent_plan(&conn);
        let candidates =
            personalized_candidates(&conn, fetched_candidates, exploration, plan.as_ref())?;
        let difficulty = effective_difficulty(&conn, plan.as_ref())?;
        let used_urls = conn
            .prepare("SELECT url FROM articles")?
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .collect();
        (candidates, difficulty, used_urls)
    };
    let mut article = match fetch_kaiyou_article_from_candidates(candidates.clone(), &used_urls).await {
        Ok(article) => article,
        Err(_) => {
            // The homepage only exposes a small, volatile slice of stories. When its
            // few long editorial pieces are already read, walk recent numeric article
            // IDs to recover an unseen public long-form article without recycling history.
            let highest_id = candidates.iter().filter_map(|candidate| {
                candidate.url.rsplit('/').next()?.parse::<u64>().ok()
            }).max().unwrap_or_default();
            let archive_candidates = (1..=180)
                .map(|offset| highest_id.saturating_sub(offset))
                .filter(|id| *id > 0)
                .map(|id| TitleCandidate {
                    id: format!("article-{id}"),
                    title: "KAI-YOU 近期归档文章".into(),
                    url: format!("https://kai-you.net/article/{id}"),
                    source: "KAI-YOU".into(),
                })
                .collect();
            fetch_kaiyou_article_from_candidates(archive_candidates, &used_urls).await?
        }
    };
    article.difficulty = difficulty;
    article.is_exploration = exploration;
    Ok(article)
}

async fn fetch_personalized_kaiyou_article_with_retry(
    db_path: &PathBuf,
) -> Result<Article, AppError> {
    let mut last_error = None;
    let backoff = [2u64, 4, 8];
    for attempt in 0..4 {
        match fetch_personalized_kaiyou_article(db_path).await {
            Ok(article) => return Ok(article),
            Err(error) => {
                last_error = Some(error);
                if attempt < 3 {
                    let delay = backoff.get(attempt).copied().unwrap_or(8);
                    std::thread::sleep(Duration::from_secs(delay));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| AppError::Message("没有找到可用的真实文章".into())))
}

async fn fetch_kaiyou_article_excluding(
    excluded_urls: &HashSet<String>,
) -> Result<Article, AppError> {
    fetch_kaiyou_article_from_candidates(fetch_kaiyou_candidates().await?, excluded_urls).await
}

fn parse_kaiyou_article_page(
    candidate: &TitleCandidate,
    page: &str,
) -> Result<Option<Article>, AppError> {
    let paragraph_selector = Selector::parse(
        ".m-article-text-main.is-normal > p, .m-article-text-main.is-normal > div > p",
    )
    .map_err(|_| AppError::Message("无法解析文章正文".into()))?;
    let image_selector = Selector::parse(
        ".m-article-text-main.is-normal img, .m-article-text-main.is-normal [data-iesrc]",
    )
    .map_err(|_| AppError::Message("无法解析文章图片".into()))?;
    let iframe_selector = Selector::parse(
        ".m-article-text-main.is-normal iframe, .m-article-text-main.is-normal [data-video]",
    )
    .map_err(|_| AppError::Message("无法解析文章嵌入内容".into()))?;
    let social_link_selector = Selector::parse(".m-article-text-main.is-normal blockquote a[href]")
        .map_err(|_| AppError::Message("无法解析社媒嵌入内容".into()))?;
    let published_selector =
        Selector::parse("time[datetime], meta[property='article:published_time']")
            .map_err(|_| AppError::Message("无法解析文章发布日期".into()))?;
    let author_selector = Selector::parse(".m-article-data-author")
        .map_err(|_| AppError::Message("无法解析文章作者".into()))?;
    let article_document = Html::parse_document(page);
    let is_editorial = article_document
        .select(&author_selector)
        .any(|node| node.text().collect::<String>().contains("KAI-YOU編集部"));
    if !is_editorial {
        return Ok(None);
    }
    let paragraphs = article_document
        .select(&paragraph_selector)
        .map(|p| p.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .filter(|text| text.chars().count() >= 24)
        .take(40)
        .collect::<Vec<_>>();
    let character_count = paragraphs
        .iter()
        .map(|paragraph| paragraph.chars().count())
        .sum::<usize>();
    if paragraphs.len() < 5 || character_count < 1200 {
        return Ok(None);
    }
    let images = article_document
        .select(&image_selector)
        .filter_map(|image| {
            image
                .value()
                .attr("data-iesrc")
                .or_else(|| image.value().attr("data-src"))
                .or_else(|| image.value().attr("src"))
        })
        .filter_map(|src| {
            url::Url::parse(&candidate.url)
                .ok()?
                .join(src)
                .ok()
                .map(|url| url.to_string())
        })
        .filter(|src| src.starts_with("https://"))
        .take(6)
        .collect::<Vec<_>>();
    let mut embed_urls = HashSet::new();
    let mut embeds = Vec::new();
    for raw_url in article_document
        .select(&iframe_selector)
        .filter_map(|node| {
            node.value()
                .attr("data-video")
                .or_else(|| node.value().attr("src"))
        })
        .chain(
            article_document
                .select(&social_link_selector)
                .filter_map(|node| node.value().attr("href")),
        )
    {
        let Some(url) = url::Url::parse(&candidate.url)
            .ok()
            .and_then(|base| base.join(raw_url).ok())
            .map(|url| url.to_string())
        else {
            continue;
        };
        if !url.starts_with("https://") || !embed_urls.insert(url.clone()) {
            continue;
        }
        let kind = if url.contains("youtube.com") || url.contains("youtu.be") {
            "video"
        } else {
            "social"
        };
        embeds.push(MediaEmbed {
            kind: kind.into(),
            url,
        });
        if embeds.len() >= 5 {
            break;
        }
    }
    let title_selector = Selector::parse("meta[property='og:title']")
        .map_err(|_| AppError::Message("无法解析文章标题".into()))?;
    let title = article_document.select(&title_selector)
        .find_map(|node| node.value().attr("content"))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| candidate.title.clone());
    let published_at = article_document
        .select(&published_selector)
        .find_map(|node| {
            node.value()
                .attr("datetime")
                .or_else(|| node.value().attr("content"))
                .map(|value| value.chars().take(10).collect::<String>())
        })
        .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
    Ok(Some(Article {
        id: format!("kaiyou-{}", Uuid::new_v4()),
        title,
        source: "KAI-YOU".into(),
        url: candidate.url.clone(),
        published_at,
        paragraphs,
        images,
        embeds,
        reading_minutes: 10,
        difficulty: "待 AI 评估".into(),
        is_exploration: false,
    }))
}

async fn fetch_kaiyou_article_from_candidates(
    candidates: Vec<TitleCandidate>,
    excluded_urls: &HashSet<String>,
) -> Result<Article, AppError> {
    let client = Client::builder()
        .user_agent("NihongoDailyReader/0.1 (personal learning reader)")
        .build()?;
    let candidates = candidates.into_iter()
        .filter(|candidate| !excluded_urls.contains(&candidate.url))
        .collect::<Vec<_>>();
    let mut pages = stream::iter(candidates.into_iter().map(|candidate| {
        let client = client.clone();
        async move {
            let page = match client.get(&candidate.url).send().await
                .and_then(|response| response.error_for_status()) {
                Ok(response) => response.text().await,
                Err(error) => Err(error),
            };
            (candidate, page)
        }
    })).buffer_unordered(6);
    let mut last_fetch_error: Option<String> = None;
    while let Some((candidate, page)) = pages.next().await {
        let page = match page {
            Ok(page) => page,
            Err(error) => { last_fetch_error = Some(error.to_string()); continue; }
        };
        if let Some(article) = parse_kaiyou_article_page(&candidate, &page)? {
            return Ok(article);
        }
    }
    Err(AppError::Message(match last_fetch_error {
        Some(error) => format!(
            "没有找到未读且正文足够长的 KAI-YOU 文章（最后一次抓取失败：{error}）"
        ),
        None => "没有找到未读且正文足够长的 KAI-YOU 文章".into(),
    }))
}

#[tauri::command]
async fn get_title_candidates() -> Result<Vec<TitleCandidate>, AppError> {
    fetch_kaiyou_candidates().await
}

#[tauri::command]
fn save_title_vote(
    state: State<'_, AppState>,
    candidate: TitleCandidate,
    vote: String,
) -> Result<(), AppError> {
    let conn = open_db(&state)?;
    conn.execute(
    "INSERT INTO title_votes (id, candidate_id, title, url, vote, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    params![Uuid::new_v4().to_string(), candidate.id, candidate.title, candidate.url, vote, Local::now().to_rfc3339()],
  )?;
    Ok(())
}

#[tauri::command]
fn get_initial_assessment() -> Vec<AssessmentQuestion> {
    initial_assessment()
}

#[tauri::command]
fn submit_initial_assessment(
    state: State<'_, AppState>,
    answers: Vec<usize>,
) -> Result<AssessmentResult, AppError> {
    let questions = initial_assessment();
    let score = answers
        .iter()
        .enumerate()
        .filter(|(index, answer)| {
            questions
                .get(*index)
                .is_some_and(|question| question.answer_index == **answer)
        })
        .count();
    let total = questions.len();
    let level_hint = match score {
        0..=3 => "当前建议从 N5–N4 难度起步；AI 会优先提供更短的日语提示。",
        4..=7 => "当前建议从 N4–N3 难度起步；先建立稳定的真实文章阅读习惯。",
        8..=10 => "当前建议从 N3 难度起步；可逐步加入更长的评论与访谈。",
        _ => "当前建议从 N3–N2 难度起步；AI 会更重视语境、篇章推理和自然表达。",
    }
    .into();
    let conn = open_db(&state)?;
    conn.execute(
    "INSERT INTO assessments (id, mode, score, total, completed_at) VALUES (?1, 'initial', ?2, ?3, ?4)",
    params![Uuid::new_v4().to_string(), score as i64, total as i64, Local::now().to_rfc3339()],
  )?;
    Ok(AssessmentResult {
        score,
        total,
        level_hint,
    })
}

#[tauri::command]
async fn get_today_article(state: State<'_, AppState>) -> Result<Article, AppError> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let existing = {
        let conn = open_db(&state)?;
        conn.execute(
            "UPDATE articles SET paragraphs_json = '[]', images_json = '[]' WHERE day < ?1",
            params![&today],
        )?;
        conn.query_row(
            "SELECT * FROM articles WHERE day = ?1 ORDER BY rowid DESC LIMIT 1",
            params![&today],
            article_from_row,
        )
        .optional()?
    };
    if let Some(article) = existing
        .as_ref()
        .filter(|article| !is_sample_article(article))
    {
        return Ok(article.clone());
    }

    let db_path = state
        .db_path
        .lock()
        .map_err(|_| AppError::Message("无法访问本地学习数据库".into()))?
        .clone();
    let article = fetch_personalized_kaiyou_article_with_retry(&db_path)
        .await
        .map_err(|error| {
            AppError::Message(format!("今天的真实文章暂未准备成功：{error}。请稍后重试。"))
        })?;
    let conn = open_db(&state)?;
    let existing = conn.query_row(
        "SELECT * FROM articles WHERE day = ?1 ORDER BY rowid DESC LIMIT 1",
        params![&today],
        article_from_row,
    );
    if let Ok(existing_article) = existing {
        if !is_sample_article(&existing_article) {
            return Ok(existing_article);
        }
        remove_article_and_learning_artifacts(&conn, &existing_article.id)?;
    }
    save_article(&conn, &article)?;
    Ok(article)
}

#[tauri::command]
async fn refresh_today_article(state: State<'_, AppState>) -> Result<Article, AppError> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let current = {
        let conn = open_db(&state)?;
        conn.query_row(
            "SELECT id, completed_at FROM articles WHERE day = ?1 ORDER BY rowid DESC LIMIT 1",
            params![&today],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?
    };
    let Some((current_id, completed_at)) = current else {
        return get_today_article(state).await;
    };
    if completed_at.is_some() {
        return Err(AppError::Message(
            "今天的日报已经完成，不能刷新，以免破坏学习记录。".into(),
        ));
    }

    // Fetch before removing the current task. If no suitable unseen article is available,
    // the user keeps the existing one instead of receiving an empty reader.
    let db_path = state
        .db_path
        .lock()
        .map_err(|_| AppError::Message("无法访问本地学习数据库".into()))?
        .clone();
    let replacement = fetch_personalized_kaiyou_article_with_retry(&db_path).await?;
    let conn = open_db(&state)?;
    remove_article_and_learning_artifacts(&conn, &current_id)?;
    save_article(&conn, &replacement)?;
    Ok(replacement)
}

fn next_due_review_card(conn: &Connection) -> Result<Option<ReviewCard>, AppError> {
    conn.query_row(
        "SELECT c.id, c.front, c.reading, c.translation, c.context_note, a.title, c.review_count
         FROM review_cards c JOIN articles a ON a.id = c.article_id
         WHERE c.due_at <= ?1 ORDER BY c.due_at ASC, c.created_at ASC LIMIT 1",
        params![Local::now().to_rfc3339()],
        |row| Ok(ReviewCard {
            id: row.get(0)?, front: row.get(1)?, reading: row.get(2)?, translation: row.get(3)?,
            context_note: row.get(4)?, article_title: row.get(5)?, review_count: row.get(6)?,
        }),
    ).optional().map_err(AppError::from)
}

#[tauri::command]
fn get_review_cards(state: State<'_, AppState>) -> Result<Vec<ReviewCard>, AppError> {
    let conn = open_db(&state)?;
    let mut statement = conn.prepare(
        "SELECT c.id, c.front, c.reading, c.translation, c.context_note, a.title, c.review_count
         FROM review_cards c JOIN articles a ON a.id = c.article_id
         ORDER BY c.due_at ASC, c.created_at DESC LIMIT 60"
    )?;
    let cards = statement.query_map([], |row| Ok(ReviewCard {
        id: row.get(0)?, front: row.get(1)?, reading: row.get(2)?, translation: row.get(3)?,
        context_note: row.get(4)?, article_title: row.get(5)?, review_count: row.get(6)?,
    }))?.filter_map(Result::ok).collect::<Vec<_>>();
    Ok(cards)
}

fn save_review_cards(conn: &Connection, selection_id: &str, article_id: &str, candidates: &[ReviewCardDraft]) -> Result<(), AppError> {
    let now = Local::now();
    for (index, candidate) in candidates.iter().enumerate() {
        let front = candidate.front.trim();
        if front.chars().count() < 2 || front.chars().count() > 30 { continue; }
        let duplicate: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM review_cards WHERE front = ?1 COLLATE NOCASE)", params![front], |row| row.get(0)
        )?;
        if duplicate { continue; }
        conn.execute(
            "INSERT INTO review_cards (id, selection_id, article_id, front, reading, translation, context_note, due_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![Uuid::new_v4().to_string(), format!("{selection_id}-{index}"), article_id, front, candidate.reading.trim(), candidate.translation.trim(), candidate.context_note.trim(), (now + ChronoDuration::days(1)).to_rfc3339(), now.to_rfc3339()],
        )?;
    }
    Ok(())
}

fn extract_plain_section(text: &str, labels: &[&str], all_labels: &[&str]) -> Option<String> {
    let (start, label_len) = labels.iter().filter_map(|label| text.find(label).map(|index| (index, label.len())))
        .min_by_key(|(index, _)| *index)?;
    let content_start = start + label_len;
    let end = all_labels.iter().filter_map(|label| text[content_start..].find(label).map(|index| content_start + index))
        .min().unwrap_or(text.len());
    let value = text[content_start..end].trim().trim_start_matches(['：', ':', ' ']).trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn explanation_from_plain_text(selection: &str, text: &str) -> Explanation {
    let reading_labels = ["[READING]", "【读音】", "读音：", "读音", "読み：", "読み"];
    let translation_labels = ["[TRANSLATION]", "【译文】", "译文：", "译文", "意思：", "意思"];
    let context_labels = ["[CONTEXT]", "【语境】", "语境：", "语境"];
    let all_labels = reading_labels.iter().chain(translation_labels.iter()).chain(context_labels.iter()).copied().collect::<Vec<_>>();
    let reading = extract_plain_section(text, &reading_labels, &all_labels).unwrap_or_else(|| selection.to_owned());
    let translation = extract_plain_section(text, &translation_labels, &all_labels).unwrap_or_else(|| text.trim().to_owned());
    let context_note = extract_plain_section(text, &context_labels, &all_labels).unwrap_or_default();
    Explanation {
        reading, translation, context_note,
        example: String::new(), example_translation: String::new(), grammar_note: String::new(), review_candidates: vec![],
    }
}

async fn generate_plain_explanation(api_key: &str, selection: &str, context: &str) -> Result<Explanation, AppError> {
    let instruction = format!(
        "请解释日语表达「{selection}」在指定语境中的意思。不要返回 JSON，只按以下三段输出，不得添加其他标题：\n[READING] 整个划选表达的纯假名读音\n[TRANSLATION] 结合上下文的简洁中文译文\n[CONTEXT] 必要时补一句中文语境提示，不需要则留空\n指定语境：{context}"
    );
    let client = api_http_client()?;
    let body: serde_json::Value = if get_protocol() == "chat_completions" {
        let response = client.post(chat_completions_url(&get_base_url())?)
            .bearer_auth(api_key)
            .json(&serde_json::json!({
                "model": get_model(), "max_tokens": 600,
                "messages": [{ "role": "user", "content": instruction }]
            })).send().await?;
        if !response.status().is_success() {
            return Err(AppError::Message(format!("纯文本解释请求失败：{}", response.status())));
        }
        response.json().await?
    } else {
        let response = client.post(responses_url(&get_base_url())?)
            .bearer_auth(api_key)
            .json(&serde_json::json!({
                "model": get_model(),
                "input": [{ "role": "user", "content": [{ "type": "input_text", "text": instruction }] }]
            })).send().await?;
        if !response.status().is_success() {
            return Err(AppError::Message(format!("纯文本解释请求失败：{}", response.status())));
        }
        response.json().await?
    };
    let text = if get_protocol() == "chat_completions" {
        body.get("choices").and_then(|value| value.as_array()).and_then(|value| value.first())
            .and_then(|value| value.get("message")).and_then(|value| value.get("content"))
            .and_then(chat_content_text)
    } else {
        responses_content_text(&body)
    }.filter(|text| !text.trim().is_empty())
        .ok_or_else(|| AppError::Message("AI 纯文本解释也返回了空内容".into()))?;
    Ok(explanation_from_plain_text(selection, &text))
}

#[tauri::command]
fn get_due_review_card(state: State<'_, AppState>) -> Result<Option<ReviewCard>, AppError> {
    let conn = open_db(&state)?;
    next_due_review_card(&conn)
}

#[tauri::command]
fn review_card(state: State<'_, AppState>, card_id: String, remembered: bool) -> Result<Option<ReviewCard>, AppError> {
    let conn = open_db(&state)?;
    let (review_count, previous_interval): (i64, i64) = conn.query_row(
        "SELECT review_count, interval_days FROM review_cards WHERE id = ?1", params![card_id], |row| Ok((row.get(0)?, row.get(1)?))
    )?;
    let interval_days = if remembered {
        match review_count {
            0 => 1,
            1 => 3,
            2 => 7,
            3 => 14,
            _ => (previous_interval * 2).min(60),
        }
    } else { 1 };
    let now = Local::now();
    conn.execute(
        "UPDATE review_cards SET review_count = review_count + 1, interval_days = ?1, due_at = ?2, last_reviewed_at = ?3 WHERE id = ?4",
        params![interval_days, (now + ChronoDuration::days(interval_days)).to_rfc3339(), now.to_rfc3339(), card_id],
    )?;
    next_due_review_card(&conn)
}

#[tauri::command]
async fn explain_selection(
    state: State<'_, AppState>,
    article_id: String,
    selection: String,
    context: String,
) -> Result<Explanation, AppError> {
    let selection_id = Uuid::new_v4().to_string();
    let created_at = Local::now().to_rfc3339();
    {
        let conn = open_db(&state)?;
        conn.execute(
        "INSERT INTO selections (id, article_id, selection, context, chinese_revealed, created_at)
     VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        params![
            selection_id,
            article_id,
            selection,
            context,
            created_at
        ],
        )?;
    }

    let selection = selection.trim();
    let existing_cards = {
        let conn = open_db(&state)?;
        let mut statement = conn.prepare("SELECT front FROM review_cards ORDER BY created_at DESC LIMIT 60")?;
        let cards = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        cards
    };
    let explanation = if let Some(api_key) = get_api_key() {
        let system = "你是面向中国母语者的日语阅读教练。用户划选了真实日文文章中的一段。不要输出日语释义。translation、contextNote、exampleTranslation、grammarNote 必须用简洁中文。reading 必须完整复现用户划选文本，并为其中每个汉字词补括号假名，例如「文化祭（ぶんかさい）のような」；绝不使用罗马音。translation 必须结合完整上下文。为提高即时反馈速度：translation 最多 28 个汉字，contextNote 最多 35 个汉字；example 最多 40 个日文字符，exampleTranslation 最多 30 个汉字；grammarNote 最多 35 个汉字。没有必要的例句或语法时，对应字段返回空字符串。不要编造原文事实。reviewCandidates 是给间隔复习用的候选卡：只保留可复用、存在学习价值的核心词、短语或语法搭配；整句、专有名词、过于简单的词、与已有卡重复或语义近重复时返回空数组。必要时把用户的长划选拆成最多两张独立的短表达卡；front 必须是原文中连续出现的日语表达。";
        let user = format!("划选文本：{selection}\n完整所在句/上下文：{context}\n已有卡正面（避免重复）：{}", if existing_cards.is_empty() { "暂无".into() } else { existing_cards.join("、") });
        match generate_structured(
            &api_key,
            system,
            &user,
            "selection_explanation",
            explanation_schema(),
        )
        .await {
            Ok(explanation) => explanation,
            Err(_) => generate_plain_explanation(&api_key, selection, &context).await?,
        }
    } else {
        Explanation {
            reading: format!("{selection}（需要配置 AI 才能生成准确假名读音）"),
            translation: "当前未配置 AI，无法生成结合上下文的准确译文。".into(),
            context_note: "请先配置 AI，再重新划选该片段。".into(),
            example: "例句需要结合具体表达生成。".into(),
            example_translation: "配置 AI 后会给出对应例句翻译。".into(),
            grammar_note: "配置 AI 后会补充相关语法或搭配。".into(),
            review_candidates: vec![],
        }
    };
    let conn = open_db(&state)?;
    save_review_cards(&conn, &selection_id, &article_id, &explanation.review_candidates)?;
    Ok(explanation)
}

#[tauri::command]
fn get_ai_status() -> AiStatus {
    AiStatus {
        configured: ai_is_configured_without_keychain_access(),
        model: get_model(),
        base_url: get_base_url(),
        protocol: get_protocol(),
    }
}

#[tauri::command]
async fn discover_models(base_url: String, api_key: String) -> Result<Vec<String>, AppError> {
    let cleaned_key = api_key.trim();
    let resolved_key = if cleaned_key.is_empty() {
        get_api_key().ok_or_else(|| AppError::Message("请先填写 API Key".into()))?
    } else {
        cleaned_key.to_string()
    };
    let endpoint = models_url(&base_url)?;
    let response = Client::new()
        .get(endpoint)
        .bearer_auth(resolved_key)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(AppError::Message(format!(
            "检测模型失败：{}",
            response.status()
        )));
    }
    let body: serde_json::Value = response.json().await?;
    let mut models: Vec<String> = body
        .get("data")
        .and_then(|data| data.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("id")
                .or_else(|| item.get("name"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .collect();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err(AppError::Message(
            "接口未返回可选模型（期望 GET /models 返回 data[].id）".into(),
        ));
    }
    Ok(models)
}

#[tauri::command]
fn save_openai_api_key(
    api_key: String,
    base_url: String,
    model: String,
    protocol: String,
) -> Result<(), AppError> {
    let cleaned = api_key.trim();
    if cleaned.is_empty() && get_api_key().is_none() {
        return Err(AppError::Message("请先填写 API Key".into()));
    }
    if !cleaned.is_empty() && cleaned.len() < 20 {
        return Err(AppError::Message("API Key 格式看起来不正确".into()));
    }
    let normalized_base_url = base_url.trim().trim_end_matches('/');
    responses_url(normalized_base_url)?;
    let normalized_model = model.trim();
    let normalized_protocol = protocol.trim();
    if !matches!(normalized_protocol, "responses" | "chat_completions") {
        return Err(AppError::Message(
            "协议必须是 Responses 或 Chat Completions".into(),
        ));
    }
    if normalized_model.is_empty()
        || normalized_model.len() > 120
        || normalized_model.chars().any(char::is_whitespace)
    {
        return Err(AppError::Message("模型名称不能为空，且不能包含空格".into()));
    }
    let stored_api_key = if cleaned.is_empty() { get_api_key().unwrap_or_default() } else { cleaned.to_owned() };
    save_local_ai_settings(LocalAiSettings {
        base_url: Some(normalized_base_url.to_owned()),
        model: Some(normalized_model.to_owned()),
        protocol: Some(normalized_protocol.to_owned()),
        api_key: Some(stored_api_key),
    })?;
    Ok(())
}

#[tauri::command]
async fn generate_questions(
    article: Article,
    learned_expressions: Vec<String>,
    question_brief: Option<String>,
) -> Result<Vec<Question>, AppError> {
    if let Some(api_key) = get_api_key() {
        let system = "你是日语阅读测验设计者。根据用户刚读完的真实文章生成恰好三道日语四选一理解题。prompt 和 choices 必须是日语；正确答案必须仅凭文章得出；evidence 必须逐字引用文章中支持正确答案的一段。错误选项要合理但不能被原文支持。explanation 必须用简洁中文解释正确理由，并且必须以“解析：”开头。不要问文章外知识，不要剧透。如果提供了历史划词表达，并且某题确实需要理解该表达才能回答，把该表达原样写入 testedExpressions；否则必须为空数组，禁止虚假绑定。";
        let user = format!(
            "文章标题：{}\n学习出题策略：{}\n在本文再次出现的历史划词表达：{}\n文章正文：\n{}",
            article.title,
            question_brief.unwrap_or_else(|| "优先验证文章主旨、语境理解和可复用表达。".into()),
            serde_json::to_string(&learned_expressions).unwrap_or_else(|_| "[]".into()),
            article.paragraphs.join("\n\n")
        );
        let mut result: QuestionSet = generate_structured(
            &api_key,
            system,
            &user,
            "reading_questions",
            question_schema(),
        )
        .await?;
        ensure_question_ids(&mut result.questions);
        if result.questions.iter().all(|question| {
            article
                .paragraphs
                .iter()
                .any(|paragraph| paragraph.contains(&question.evidence))
                && question.tested_expressions.iter().all(|expression| {
                    learned_expressions.contains(expression)
                        && question.evidence.contains(expression)
                })
        }) {
            return Ok(result.questions);
        }
        return Err(AppError::Message(
            "AI 题目证据未能在原文中验证，已拒绝使用".into(),
        ));
    }
    Ok(fallback_questions(&article))
}

#[tauri::command]
async fn get_questions(
    state: State<'_, AppState>,
    article: Article,
) -> Result<Vec<Question>, AppError> {
    {
        let conn = open_db(&state)?;
        if let Some(stored) = conn
            .query_row(
                "SELECT questions_json FROM article_questions WHERE article_id = ?1",
                params![article.id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            let questions: Vec<Question> = serde_json::from_str(&stored)
                .map_err(|_| AppError::Message("已保存的理解题数据损坏".into()))?;
            if questions
                .iter()
                .all(|question| question.explanation.starts_with("解析："))
            {
                return Ok(questions);
            }
        }
    }
    let text = article.paragraphs.join("\n");
    let learned_expressions = {
        let conn = open_db(&state)?;
        let mut statement = conn.prepare(
      "SELECT DISTINCT selection FROM selections WHERE article_id <> ?1 AND length(selection) BETWEEN 2 AND 30 ORDER BY created_at DESC"
    )?;
        let expressions = statement
            .query_map(params![article.id], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter(|expression| text.contains(expression))
            .take(8)
            .collect::<Vec<_>>();
        expressions
    };
    let question_brief = {
        let conn = open_db(&state)?;
        multi_agent_plan(&conn).map(|plan| plan.question_brief)
    };
    let questions = match generate_questions(article.clone(), learned_expressions, question_brief).await {
        Ok(questions) => questions,
        Err(_) => return Ok(fallback_questions(&article)),
    };
    let conn = open_db(&state)?;
    conn.execute(
    "INSERT OR REPLACE INTO article_questions (article_id, questions_json, generated_at) VALUES (?1, ?2, ?3)",
    params![article.id, serde_json::to_string(&questions).map_err(|error| AppError::Message(format!("理解题保存失败：{error}")))?, Local::now().to_rfc3339()]
  )?;
    Ok(questions)
}

fn fallback_questions(article: &Article) -> Vec<Question> {
    let len = article.paragraphs.len();
    let indices = [0, len.saturating_sub(1) / 2, len.saturating_sub(1)];
    indices.into_iter().enumerate().map(|(number, index)| {
        let evidence = article.paragraphs.get(index).cloned().unwrap_or_default();
        Question {
            id: format!("local-evidence-{}", number + 1),
            prompt: format!("本文の内容として、原文で直接確認できるものはどれですか。（{}）", number + 1),
            choices: vec![
                evidence.clone(),
                "本文では、この記事の出来事がすべて中止されたと説明している。".into(),
                "本文では、関係者が情報を全面的に否定したと述べている。".into(),
                "本文では、この話題と無関係な出来事だけを紹介している。".into(),
            ],
            answer_index: 0,
            evidence,
            explanation: "解析：正确选项直接引用自本文；其他选项没有原文依据。AI 出题服务恢复后可重新生成更有区分度的题目。".into(),
            tested_expressions: vec![],
        }
    }).collect()
}

fn current_week() -> String {
    let now = Local::now();
    format!("{}-W{:02}", now.iso_week().year(), now.iso_week().week())
}

#[tauri::command]
async fn get_weekly_assessment(state: State<'_, AppState>) -> Result<WeeklyAssessment, AppError> {
    let week = current_week();
    let existing = {
        let conn = open_db(&state)?;
        conn.query_row(
      "SELECT id, article_json, questions_json, score, total, completed_at FROM weekly_assessments WHERE week = ?1",
      params![week],
      |row| {
        let article_json: String = row.get(1)?;
        let questions_json: String = row.get(2)?;
        let score: Option<usize> = row.get(3)?;
        let total: Option<usize> = row.get(4)?;
        let completed_at: Option<String> = row.get(5)?;
        Ok((row.get::<_, String>(0)?, article_json, questions_json, score, total, completed_at))
      },
    ).optional()?
    };
    if let Some((id, article_json, questions_json, score, total, completed_at)) = existing {
        let article = serde_json::from_str(&article_json)
            .map_err(|_| AppError::Message("本周评估文章损坏".into()))?;
        let questions = serde_json::from_str(&questions_json)
            .map_err(|_| AppError::Message("本周评估题目损坏".into()))?;
        let result = score.zip(total).map(|(score, total)| AssessmentResult {
            score,
            total,
            level_hint: "本周独立评估已完成；下周会使用一篇新的未读文章。".into(),
        });
        return Ok(WeeklyAssessment {
            id,
            week,
            article,
            questions,
            completed: completed_at.is_some(),
            result,
        });
    }

    let used_urls = {
        let conn = open_db(&state)?;
        let mut urls: HashSet<String> = conn
            .prepare("SELECT url FROM articles")?
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .collect();
        let mut statement = conn.prepare("SELECT article_json FROM weekly_assessments")?;
        for stored in statement
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
        {
            if let Ok(article) = serde_json::from_str::<Article>(&stored) {
                urls.insert(article.url);
            }
        }
        urls
    };
    let article = fetch_kaiyou_article_excluding(&used_urls)
        .await
        .map_err(|error| {
            AppError::Message(format!(
                "本周独立评估的真实文章暂未准备成功：{error}。请稍后重试。"
            ))
        })?;
    let questions = match generate_questions(article.clone(), vec![], None).await {
        Ok(items) => items,
        Err(_) => fallback_questions(&article),
    };
    let id = format!("weekly-{}", Uuid::new_v4());
    {
        let conn = open_db(&state)?;
        conn.execute(
      "INSERT INTO weekly_assessments (id, week, article_json, questions_json) VALUES (?1, ?2, ?3, ?4)",
      params![
        id,
        week,
        serde_json::to_string(&article).map_err(|error| AppError::Message(format!("评估文章保存失败：{error}")))?,
        serde_json::to_string(&questions).map_err(|error| AppError::Message(format!("评估题目保存失败：{error}")))?
      ],
    )?;
    }
    Ok(WeeklyAssessment {
        id,
        week,
        article,
        questions,
        completed: false,
        result: None,
    })
}

#[tauri::command]
fn submit_weekly_assessment(
    state: State<'_, AppState>,
    assessment_id: String,
    answers: Vec<usize>,
) -> Result<AssessmentResult, AppError> {
    let conn = open_db(&state)?;
    let questions_json: String = conn.query_row(
        "SELECT questions_json FROM weekly_assessments WHERE id = ?1",
        params![assessment_id],
        |row| row.get(0),
    )?;
    let questions: Vec<Question> = serde_json::from_str(&questions_json)
        .map_err(|_| AppError::Message("本周评估题目损坏".into()))?;
    let score = answers
        .iter()
        .enumerate()
        .filter(|(index, answer)| {
            questions
                .get(*index)
                .is_some_and(|question| question.answer_index == **answer)
        })
        .count();
    let total = questions.len();
    conn.execute(
        "UPDATE weekly_assessments SET score = ?1, total = ?2, completed_at = ?3 WHERE id = ?4",
        params![
            score as i64,
            total as i64,
            Local::now().to_rfc3339(),
            assessment_id
        ],
    )?;
    Ok(AssessmentResult {
        score,
        total,
        level_hint: "这是无翻译、无划词辅助下的理解成绩；它将与日常阅读数据分开记录。".into(),
    })
}

#[tauri::command]
fn record_answer(
    state: State<'_, AppState>,
    article_id: String,
    question_id: String,
    chosen_index: usize,
    answer_index: usize,
    tested_expressions: Vec<String>,
) -> Result<bool, AppError> {
    let correct = chosen_index == answer_index;
    let conn = open_db(&state)?;
    conn.execute(
        "INSERT INTO answers (id, article_id, question_id, chosen_index, correct, created_at)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            Uuid::new_v4().to_string(),
            article_id,
            question_id,
            chosen_index as i64,
            correct as i64,
            Local::now().to_rfc3339()
        ],
    )?;
    for expression in tested_expressions {
        let used_assistance = conn.query_row(
      "SELECT EXISTS(SELECT 1 FROM selections WHERE article_id = ?1 AND (instr(selection, ?2) > 0 OR instr(?2, selection) > 0))",
      params![article_id, expression], |row| row.get::<_, bool>(0)
    )?;
        conn.execute(
      "INSERT OR REPLACE INTO expression_evidence (id, article_id, question_id, expression, correct, used_assistance, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      params![Uuid::new_v4().to_string(), article_id, question_id, expression, correct as i64, used_assistance as i64, Local::now().to_rfc3339()]
    )?;
    }
    Ok(correct)
}

#[tauri::command]
fn complete_article(state: State<'_, AppState>, article_id: String) -> Result<(), AppError> {
    let conn = open_db(&state)?;
    conn.execute(
        "UPDATE articles SET completed_at = ?1 WHERE id = ?2",
        params![Local::now().to_rfc3339(), article_id],
    )?;
    Ok(())
}

#[tauri::command]
fn save_topic_feedback(
    state: State<'_, AppState>,
    article_id: String,
    label: String,
) -> Result<(), AppError> {
    let conn = open_db(&state)?;
    conn.execute(
        "INSERT INTO topic_feedback (id, article_id, label, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            Uuid::new_v4().to_string(),
            article_id,
            label,
            Local::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

#[tauri::command]
fn get_progress(state: State<'_, AppState>) -> Result<Progress, AppError> {
    let conn = open_db(&state)?;
    let selected_count = conn.query_row("SELECT COUNT(*) FROM selections", [], |row| row.get(0))?;
    let chinese_reveals = conn.query_row(
        "SELECT COUNT(*) FROM selections WHERE chinese_revealed = 1",
        [],
        |row| row.get(0),
    )?;
    let completed_articles = conn.query_row(
        "SELECT COUNT(*) FROM articles WHERE completed_at IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    let today = Local::now().format("%Y-%m-%d").to_string();
    let missed_articles = conn.query_row(
        "SELECT COUNT(*) FROM articles WHERE day < ?1 AND completed_at IS NULL",
        params![today],
        |row| row.get(0),
    )?;
    let title_votes = conn.query_row("SELECT COUNT(*) FROM title_votes", [], |row| row.get(0))?;
    let baseline_completed = conn.query_row(
        "SELECT COUNT(*) FROM assessments WHERE mode = 'initial'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
    let mut statement = conn.prepare(
        "SELECT label, COUNT(*) FROM topic_feedback GROUP BY label ORDER BY COUNT(*) DESC",
    )?;
    let topic_feedback = statement
        .query_map([], |row| {
            Ok(TopicFeedback {
                label: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    let mut statement = conn.prepare(
        "SELECT a.day, a.difficulty, a.paragraphs_json, COUNT(s.id)
     FROM articles a LEFT JOIN selections s ON s.article_id = a.id
     WHERE a.day >= date('now', '-13 days') AND a.completed_at IS NOT NULL
     GROUP BY a.id ORDER BY a.day",
    )?;
    let selection_trend: Vec<SelectionTrendPoint> = statement
        .query_map([], |row| {
            let day: String = row.get(0)?;
            let difficulty: String = row.get(1)?;
            let paragraphs_json: String = row.get(2)?;
            let selections: i64 = row.get(3)?;
            let character_count = serde_json::from_str::<Vec<String>>(&paragraphs_json)
                .unwrap_or_default()
                .iter()
                .map(|item| item.chars().count() as i64)
                .sum::<i64>()
                .max(1);
            let difficulty_factor = if difficulty.contains("N1") {
                1.5
            } else if difficulty.contains("N2") {
                1.3
            } else if difficulty.contains("N3") {
                1.1
            } else if difficulty.contains("N4") {
                0.9
            } else {
                0.75
            };
            Ok(SelectionTrendPoint {
                day,
                normalized_rate: selections as f64 * 1000.0
                    / character_count as f64
                    / difficulty_factor,
                selections,
                character_count,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    let mut statement = conn.prepare("SELECT week, score, total FROM weekly_assessments WHERE completed_at IS NOT NULL ORDER BY week")?;
    let assessment_trend: Vec<AssessmentTrendPoint> = statement
        .query_map([], |row| {
            let total: i64 = row.get(2)?;
            Ok(AssessmentTrendPoint {
                week: row.get(0)?,
                score_rate: if total > 0 {
                    row.get::<_, i64>(1)? as f64 / total as f64
                } else {
                    0.0
                },
            })
        })?
        .filter_map(Result::ok)
        .collect();
    let independent_expression_attempts =
        conn.query_row("SELECT COUNT(*) FROM expression_evidence", [], |row| {
            row.get(0)
        })?;
    let independent_successes: i64 = conn.query_row(
        "SELECT COUNT(*) FROM expression_evidence WHERE correct = 1 AND used_assistance = 0",
        [],
        |row| row.get(0),
    )?;
    let independent_expression_rate = (independent_expression_attempts > 0)
        .then_some(independent_successes as f64 / independent_expression_attempts as f64);
    let completed_days: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT day) FROM articles WHERE completed_at IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    let first_completed_day: Option<String> = conn.query_row(
        "SELECT MIN(day) FROM articles WHERE completed_at IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    let observed_days = first_completed_day
        .and_then(|day| NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok())
        .map(|day| (Local::now().date_naive() - day).num_days() + 1)
        .unwrap_or(0);
    let selection_rate_change = split_change(
        selection_trend
            .iter()
            .map(|point| point.normalized_rate)
            .collect(),
        3,
    );
    let weekly_score_non_declining = if assessment_trend.len() >= 2 {
        Some(
            assessment_trend.last().unwrap().score_rate
                >= assessment_trend.first().unwrap().score_rate,
        )
    } else {
        None
    };
    let expression_rows = {
        let mut statement = conn.prepare(
            "SELECT correct, used_assistance FROM expression_evidence ORDER BY created_at",
        )?;
        let values = statement
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
            .filter_map(Result::ok)
            .map(|(correct, assistance)| {
                if correct == 1 && assistance == 0 {
                    1.0
                } else {
                    0.0
                }
            })
            .collect::<Vec<_>>();
        values
    };
    let expression_rate_change = split_change(expression_rows, 4);
    let ready_for_verdict = observed_days >= 14
        && completed_days >= 10
        && selection_rate_change.is_some()
        && weekly_score_non_declining.is_some()
        && expression_rate_change.is_some();
    let verdict = if !ready_for_verdict {
        "证据积累中".into()
    } else if selection_rate_change.is_some_and(|change| change < 0.0)
        && weekly_score_non_declining == Some(true)
        && expression_rate_change.is_some_and(|change| change > 0.0)
    {
        "两周试验达到三项有效标准".into()
    } else {
        "两周试验尚未同时达到三项标准".into()
    };
    let experiment = ExperimentStatus {
        observed_days,
        completed_days,
        selection_rate_change,
        weekly_score_non_declining,
        expression_rate_change,
        ready_for_verdict,
        verdict,
    };
    Ok(Progress {
        selected_count,
        chinese_reveals,
        completed_articles,
        missed_articles,
        title_votes,
        baseline_completed,
        topic_feedback,
        selection_trend,
        assessment_trend,
        independent_expression_rate,
        independent_expression_attempts,
        experiment,
    })
}

fn split_change(values: Vec<f64>, minimum_per_half: usize) -> Option<f64> {
    if values.len() < minimum_per_half * 2 {
        return None;
    }
    let split = values.len() / 2;
    let first = values[..split].iter().sum::<f64>() / split as f64;
    let second = values[split..].iter().sum::<f64>() / (values.len() - split) as f64;
    Some(second - first)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
            fs::create_dir_all(&data_dir)?;
            app.manage(AppState {
                db_path: Mutex::new(data_dir.join("learning.sqlite3")),
            });
            if std::env::args().any(|argument| argument == "--daily-reminder") {
                let db_path = data_dir.join("learning.sqlite3");
                let existing = Connection::open(&db_path).ok().and_then(|conn| {
                    conn.query_row(
          "SELECT * FROM articles WHERE day = date('now', 'localtime') ORDER BY rowid DESC LIMIT 1",
          [],
          article_from_row,
        ).ok()
                });
                let mut title = existing
                    .as_ref()
                    .filter(|article| !is_sample_article(article))
                    .map(|article| article.title.clone());
                if title.is_none() {
                    // The reminder may be the first process to create today's task. It must use the
                    // same history-aware selection path as the reader; otherwise it can re-save
                    // yesterday's top homepage article before the main app has a chance to filter it.
                    if let Ok(article) = tauri::async_runtime::block_on(
                        fetch_personalized_kaiyou_article_with_retry(&db_path),
                    ) {
                        if let Ok(conn) = Connection::open(&db_path) {
                            if let Some(sample) = existing
                                .as_ref()
                                .filter(|article| is_sample_article(article))
                            {
                                let _ = remove_article_and_learning_artifacts(&conn, &sample.id);
                            }
                            let _ = save_article(&conn, &article);
                            title = Some(article.title);
                        }
                    }
                }
                let title = title
                    .unwrap_or_else(|| "今天的真实文章暂未准备完成，打开 App 后会自动重试".into());
                let _ = app
                    .notification()
                    .builder()
                    .title("Kotoba Atelier")
                    .body(title)
                    .show();
                app.handle().exit(0);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_today_article,
            refresh_today_article,
            get_title_candidates,
            save_title_vote,
            get_initial_assessment,
            submit_initial_assessment,
            get_weekly_assessment,
            submit_weekly_assessment,
            explain_selection,
            get_due_review_card,
            get_review_cards,
            review_card,
            get_ai_status,
            discover_models,
            save_openai_api_key,
            get_questions,
            record_answer,
            complete_article,
            save_topic_feedback,
            get_progress,
            get_reminder_status,
            install_daily_reminder,
            remove_daily_reminder,
            get_ability_profile,
            update_target_level,
            get_multi_agent_plan,
            refresh_multi_agent_plan
        ])
        .run(tauri::generate_context!())
        .expect("启动 Kotoba Atelier 失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kaiyou_candidate() -> TitleCandidate {
        TitleCandidate {
            id: "article-1".into(),
            title: "テスト記事".into(),
            url: "https://kai-you.net/article/1".into(),
            source: "KAI-YOU".into(),
        }
    }

    fn long_article_html(extra: &str) -> String {
        let paragraph = "これは日本のポップカルチャーについて詳しく説明するテスト用の文章です。読者が記事全体の流れを確認できるように、十分な長さを持つ本文として同じ話題を継続して紹介しています。".repeat(4);
        format!("<html><head><meta property='article:published_time' content='2026-07-09T10:00:00+09:00'></head><body><div class='m-article-data-author'>KAI-YOU編集部</div><div class='m-article-text-main is-normal'>{}<img src='/images/a.jpg'><div data-video='https://www.youtube.com/embed/abc'></div><blockquote><a href='https://x.com/example/status/1'>post</a></blockquote>{}</div></body></html>", (0..5).map(|_| format!("<p>{paragraph}</p>")).collect::<String>(), extra)
    }

    #[test]
    fn ai_gateway_requires_https() {
        assert!(responses_url("http://localhost:11434/v1").is_err());
        assert_eq!(
            responses_url("https://api.example.com/v1").unwrap(),
            "https://api.example.com/v1/responses"
        );
    }

    #[test]
    fn malformed_anki_candidates_do_not_break_translation() {
        let explanation: Explanation = serde_json::from_value(serde_json::json!({
            "reading": "ハック（はっく）", "translation": "改造、突破既有方式", "contextNote": "此处是比喻用法",
            "example": "街をハックする。", "exampleTranslation": "重新改造城市。", "grammarNote": "サ变动词",
            "reviewCandidates": ["ハッカー", "ハッキング"]
        })).unwrap();
        assert_eq!(explanation.translation, "改造、突破既有方式");
        assert!(explanation.review_candidates.is_empty());
    }

    #[test]
    fn json_parser_accepts_fences_and_leading_text() {
        let value = parse_json_value("回答如下：\n```json\n{\"translation\":\"测试\"}\n```").unwrap();
        assert_eq!(value["translation"], "测试");
    }

    #[test]
    fn plain_fallback_separates_kana_translation_and_context() {
        let explanation = explanation_from_plain_text(
            "見放題最速配信スタートする",
            "读音 みほうだい さいそく はいしん スタートする\n译文 从7月7日起开始最快不限次数播放。\n语境 宣传流媒体上线时间。",
        );
        assert!(explanation.reading.starts_with("みほうだい"));
        assert!(explanation.translation.starts_with("从7月7日"));
        assert_eq!(explanation.context_note, "宣传流媒体上线时间。");
        assert!(!explanation.context_note.contains("Schema"));
    }

    #[test]
    fn missing_question_id_is_generated_locally() {
        let mut set: QuestionSet = serde_json::from_value(serde_json::json!({
            "questions": [{
                "prompt": "本文の内容はどれですか。", "choices": ["A", "B", "C", "D"],
                "answerIndex": 0, "evidence": "原文", "explanation": "解析：原文にある。", "testedExpressions": []
            }]
        })).unwrap();
        ensure_question_ids(&mut set.questions);
        assert!(set.questions[0].id.starts_with("ai-question-1-"));
    }

    #[test]
    fn local_question_fallback_always_provides_three_evidence_questions() {
        let article = Article {
            id: "fallback-article".into(), title: "测试".into(), source: "test".into(), url: "https://example.com".into(),
            published_at: "2026-07-13".into(), paragraphs: vec!["第一段原文。".into(), "第二段原文。".into(), "第三段原文。".into()],
            images: vec![], embeds: vec![], reading_minutes: 1, difficulty: "N3".into(), is_exploration: false,
        };
        let questions = fallback_questions(&article);
        assert_eq!(questions.len(), 3);
        assert!(questions.iter().all(|question| article.paragraphs.contains(&question.evidence)));
    }

    #[test]
    #[ignore = "requires the locally configured AI gateway"]
    fn live_configured_gateway_returns_usable_explanation() {
        tauri::async_runtime::block_on(async {
            let api_key = get_api_key().expect("local API key must be configured");
            let explanation: Explanation = generate_structured(
                &api_key,
                "你是日语阅读教练。返回简洁中文解释。reading、translation、contextNote、example、exampleTranslation、grammarNote 必须是字符串；reviewCandidates 必须是数组，不确定时返回空数组。",
                "划选文本：ハック\n上下文：渋谷の街をハックする。",
                "selection_explanation",
                explanation_schema(),
            ).await.unwrap();
            assert!(!explanation.translation.trim().is_empty());
            assert!(!explanation.reading.trim().is_empty());
            let fallback = generate_plain_explanation(
                &api_key,
                "見放題最速配信スタートする",
                "7月7日からAmazonのPrime Videoで見放題最速配信スタートする。",
            ).await.unwrap();
            assert_ne!(fallback.reading, "見放題最速配信スタートする");
            assert!(!fallback.translation.contains("[READING]"));
            assert!(!fallback.context_note.contains("Schema"));
        });
    }

    #[test]
    #[ignore = "requires the locally configured AI gateway and article database"]
    fn live_configured_gateway_generates_questions_with_ids() {
        tauri::async_runtime::block_on(async {
            let home = std::env::var_os("HOME").unwrap();
            let db = PathBuf::from(home).join("Library/Application Support/com.xtnntn.nihongo-daily-reader/learning.sqlite3");
            let conn = Connection::open(db).unwrap();
            let article = conn.query_row("SELECT * FROM articles ORDER BY rowid DESC LIMIT 1", [], article_from_row).unwrap();
            let questions = generate_questions(article, vec![], None).await.unwrap();
            assert_eq!(questions.len(), 3);
            assert!(questions.iter().all(|question| !question.id.trim().is_empty()));
        });
    }

    #[test]
    fn due_review_card_is_returned_in_oldest_due_order() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE articles (id TEXT PRIMARY KEY, title TEXT NOT NULL);
             CREATE TABLE review_cards (
               id TEXT PRIMARY KEY, selection_id TEXT UNIQUE, article_id TEXT, front TEXT, reading TEXT,
               translation TEXT, context_note TEXT, review_count INTEGER, interval_days INTEGER,
               due_at TEXT, last_reviewed_at TEXT, created_at TEXT
             );
             INSERT INTO articles VALUES ('a1', '旧文章');
             INSERT INTO review_cards VALUES ('card-1', 'selection-1', 'a1', '企画', '企画（きかく）', '策划', '文章中的项目安排', 2, 3, '2000-01-01T00:00:00+00:00', NULL, '1999-01-01T00:00:00+00:00');",
        ).unwrap();
        let card = next_due_review_card(&conn).unwrap().unwrap();
        assert_eq!(card.id, "card-1");
        assert_eq!(card.front, "企画");
        assert_eq!(card.review_count, 2);
    }

    #[test]
    fn candidate_ranking_uses_interest_signals_and_exploration_direction() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE title_votes (title TEXT NOT NULL, vote TEXT NOT NULL);
       CREATE TABLE articles (id TEXT PRIMARY KEY, title TEXT NOT NULL);
       CREATE TABLE topic_feedback (article_id TEXT NOT NULL, label TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO title_votes (title, vote) VALUES (?1, '想看')",
            ["VTuber配信ライブの新企画"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO title_votes (title, vote) VALUES (?1, '不想看')",
            ["スポーツ大会の試合結果"],
        )
        .unwrap();
        let candidates = vec![
            TitleCandidate {
                id: "sports".into(),
                title: "スポーツ大会の最新結果".into(),
                url: "https://example.com/sports".into(),
                source: "KAI-YOU".into(),
            },
            TitleCandidate {
                id: "vtuber".into(),
                title: "VTuber配信ライブ開催".into(),
                url: "https://example.com/vtuber".into(),
                source: "KAI-YOU".into(),
            },
        ];
        let preferred = personalized_candidates(&conn, candidates.clone(), false, None).unwrap();
        assert_eq!(preferred[0].id, "vtuber");
        let exploration = personalized_candidates(&conn, candidates, true, None).unwrap();
        assert_eq!(exploration[0].id, "sports");
    }

    #[test]
    fn curator_uses_saved_agent_focus_terms() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE title_votes (title TEXT NOT NULL, vote TEXT NOT NULL);
       CREATE TABLE articles (id TEXT PRIMARY KEY, title TEXT NOT NULL);
       CREATE TABLE topic_feedback (article_id TEXT NOT NULL, label TEXT NOT NULL);",
        )
        .unwrap();
        let plan = MultiAgentPlan {
            target_difficulty: "N3".into(),
            focus_terms: vec!["VTuber".into()],
            avoid_terms: vec!["スポーツ".into()],
            article_brief: "".into(),
            question_brief: "".into(),
            rationale: "".into(),
            updated_at: "".into(),
        };
        let candidates = vec![
            TitleCandidate {
                id: "sports".into(),
                title: "スポーツ大会の最新結果".into(),
                url: "https://example.com/sports".into(),
                source: "KAI-YOU".into(),
            },
            TitleCandidate {
                id: "vtuber".into(),
                title: "VTuber配信ライブ開催".into(),
                url: "https://example.com/vtuber".into(),
                source: "KAI-YOU".into(),
            },
        ];
        let ranked = personalized_candidates(&conn, candidates, false, Some(&plan)).unwrap();
        assert_eq!(ranked[0].id, "vtuber");
    }

    #[test]
    fn manual_target_level_overrides_selection_without_rewriting_suggestion() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
       CREATE TABLE assessments (mode TEXT, score INTEGER, total INTEGER, completed_at TEXT);
       CREATE TABLE articles (id TEXT, day TEXT, paragraphs_json TEXT, completed_at TEXT);
       CREATE TABLE selections (id TEXT, article_id TEXT);
       INSERT INTO assessments VALUES ('initial', 9, 12, '2026-07-11');",
        )
        .unwrap();
        assert_eq!(automatic_difficulty(&conn).unwrap(), "N2");
        conn.execute("INSERT INTO app_settings VALUES ('target_level', 'N4')", [])
            .unwrap();
        assert_eq!(automatic_difficulty(&conn).unwrap(), "N2");
        assert_eq!(effective_difficulty(&conn, None).unwrap(), "N4");
    }

    #[test]
    fn split_change_requires_evidence_and_preserves_direction() {
        assert_eq!(
            split_change(vec![8.0, 7.0, 6.0, 4.0, 3.0, 2.0], 3),
            Some(-4.0)
        );
        assert!((split_change(vec![0.2, 0.3, 0.8, 0.9], 2).unwrap() - 0.6).abs() < 1e-9);
        assert_eq!(split_change(vec![1.0, 0.5, 0.2], 2), None);
    }

    #[test]
    fn kaiyou_parser_accepts_public_long_article_and_preserves_media() {
        let article = parse_kaiyou_article_page(&kaiyou_candidate(), &long_article_html(""))
            .unwrap()
            .unwrap();
        assert_eq!(article.published_at, "2026-07-09");
        assert_eq!(article.paragraphs.len(), 5);
        assert_eq!(article.images, vec!["https://kai-you.net/images/a.jpg"]);
        assert!(article
            .embeds
            .iter()
            .any(|embed| embed.kind == "video" && embed.url.contains("youtube.com/embed/abc")));
        assert!(article
            .embeds
            .iter()
            .any(|embed| embed.kind == "social" && embed.url.contains("x.com/example/status/1")));
    }

    #[test]
    fn kaiyou_parser_rejects_restricted_or_user_submitted_pages() {
        let user_page = long_article_html("").replace("KAI-YOU編集部", "一般ユーザー");
        let premium_page = long_article_html("").replace(
            "m-article-text-main is-normal",
            "m-article-text-main is-premium",
        );
        assert!(parse_kaiyou_article_page(&kaiyou_candidate(), &user_page)
            .unwrap()
            .is_none());
        assert!(
            parse_kaiyou_article_page(&kaiyou_candidate(), &premium_page)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn kaiyou_parser_rejects_short_article() {
        let html = "<div class='m-article-data-author'>KAI-YOU編集部</div><div class='m-article-text-main is-normal'><p>短い本文ですが二十四文字以上になるように少しだけ伸ばします。</p><p>二つ目の短い段落もここに置いておきます。</p></div>";
        assert!(parse_kaiyou_article_page(&kaiyou_candidate(), html)
            .unwrap()
            .is_none());
    }

    #[test]
    #[ignore = "requires live KAI-YOU network access"]
    fn live_kaiyou_pages_have_at_least_one_compatible_article() {
        tauri::async_runtime::block_on(async {
            let candidates = fetch_kaiyou_candidates().await.unwrap();
            assert!(candidates.len() >= 20);
            let article = fetch_kaiyou_article_from_candidates(candidates, &HashSet::new())
                .await
                .unwrap();
            assert_eq!(article.source, "KAI-YOU");
            assert!(
                article
                    .paragraphs
                    .iter()
                    .map(|paragraph| paragraph.chars().count())
                    .sum::<usize>()
                    >= 1200
            );
        });
    }

    #[test]
    #[ignore = "requires live KAI-YOU network access"]
    fn live_candidates_can_replace_a_previously_read_article() {
        tauri::async_runtime::block_on(async {
            let candidates = fetch_kaiyou_candidates().await.unwrap();
            let mut excluded = HashSet::new();
            excluded.insert("https://kai-you.net/article/95856".to_string());
            let article = fetch_kaiyou_article_from_candidates(candidates, &excluded)
                .await
                .unwrap();
            assert_ne!(article.url, "https://kai-you.net/article/95856");
            assert_eq!(article.source, "KAI-YOU");
            assert!(
                article
                    .paragraphs
                    .iter()
                    .map(|paragraph| paragraph.chars().count())
                    .sum::<usize>()
                    >= 1200
            );
        });
    }

}
