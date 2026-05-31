# Realtime Assistant Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Добавить в Meetily realtime assistant, который читает live transcript chunks, отправляет короткое окно транскрипта в выбранную LLM и показывает приватные рекомендации во время звонка.

**Architecture:** Реализация идёт как внутренний `realtime_assistant` модуль с plugin-shaped boundary: Rust-команды, чистые типы, конфиг в Tauri Store, frontend service/hook и отдельная панель. LLM routing остаётся в Rust и переиспользует существующий summary model config, чтобы API keys и provider logic не дублировались в React. По умолчанию действует local-only privacy gate: `ollama`, `builtin-ai` и localhost `custom-openai` разрешены, cloud providers требуют явного включения.

**Tech Stack:** Tauri 2, Rust, Tokio, reqwest, serde, tauri-plugin-store, Next.js 14, React 18, TypeScript, Tailwind, lucide-react, существующий `summary::llm_client`.

---

## Контекст И Границы

Текущий источник live-транскрипта уже есть:

- `frontend/src-tauri/src/audio/transcription/worker.rs` эмитит `transcript-update`.
- `frontend/src/services/transcriptService.ts` слушает `transcript-update`.
- `frontend/src/contexts/TranscriptContext.tsx` буферизует transcript state.
- `frontend/src-tauri/src/summary/llm_client.rs` поддерживает `openai`, `openrouter`, `ollama`, `custom-openai`, `builtin-ai`, `claude`, `groq`.
- `frontend/src-tauri/src/api/api.rs` и `frontend/src-tauri/src/database/repositories/setting.rs` уже умеют доставать summary model config и API keys.
- `frontend/src-tauri/src/onboarding.rs` и `frontend/src-tauri/src/audio/recording_preferences.rs` показывают текущий паттерн Tauri Store.

Выбор архитектуры: realtime assistant встраивается как internal module. Tauri plugin crate стоит вынести отдельным этапом после MVP, когда появится второй Tauri host app или публичный extension contract.

## File Structure

- Create: `frontend/src-tauri/src/realtime_assistant/mod.rs`  
  Экспортирует подмодули и публичные команды.

- Create: `frontend/src-tauri/src/realtime_assistant/types.rs`  
  Общие Rust DTO: config, transcript segment, evaluate request, recommendation response, session response.

- Create: `frontend/src-tauri/src/realtime_assistant/window.rs`  
  Чистая логика rolling window, фильтрации partial/low-confidence chunks и форматирования transcript для prompt.

- Create: `frontend/src-tauri/src/realtime_assistant/prompt.rs`  
  System/user prompt builder и очистка JSON ответа от code fences.

- Create: `frontend/src-tauri/src/realtime_assistant/service.rs`  
  Загрузка конфига из Tauri Store, privacy gate, provider resolution, вызов `summary::llm_client::generate_summary`, parsing strict JSON.

- Create: `frontend/src-tauri/src/realtime_assistant/commands.rs`  
  Tauri commands: `assistant_get_config`, `assistant_save_config`, `assistant_start_session`, `assistant_stop_session`, `assistant_evaluate_window`.

- Modify: `frontend/src-tauri/src/lib.rs`  
  Добавить `pub mod realtime_assistant;` и зарегистрировать команды в `invoke_handler`.

- Create: `frontend/src/services/realtimeAssistantService.ts`  
  TypeScript wrapper вокруг Tauri commands и event listener для `assistant-recommendations-update`.

- Create: `frontend/src/hooks/useRealtimeAssistant.ts`  
  Throttle, in-flight guard, rolling window call trigger, stale-response guard.

- Create: `frontend/src/components/RealtimeAssistantPanel.tsx`  
  Правая панель realtime recommendations на главном экране.

- Create: `frontend/src/components/RealtimeAssistantSettings.tsx`  
  Настройки assistant: enable, system prompt, local-only gate, interval/window limits.

- Modify: `frontend/src/app/page.tsx`  
  Подключить `useRealtimeAssistant` и панель рядом с `TranscriptPanel`.

- Modify: `frontend/src/app/settings/page.tsx`  
  Добавить tab `Assistant`.

- Modify: `frontend/src/types/index.ts`  
  Добавить TS-типы assistant DTO.

## Preflight

- [ ] **Step 1: Установить package manager, если отсутствует**

Run:

```bash
cd frontend
command -v pnpm || npm install -g pnpm
pnpm --version
```

Expected: команда печатает версию `pnpm`.

- [ ] **Step 2: Проверить базовую сборочную поверхность**

Run:

```bash
cargo check -p meetily
cd frontend
pnpm install
pnpm run lint
```

Expected: `cargo check` и `pnpm run lint` завершаются успешно. Если падает внешняя зависимость или текущий Tauri/Next toolchain, выполнить интернет-ресеч по первичному источнику ошибки и зафиксировать причину перед изменениями.

---

### Task 1: Backend DTO, Rolling Window, Prompt Helpers

**Files:**
- Create: `frontend/src-tauri/src/realtime_assistant/mod.rs`
- Create: `frontend/src-tauri/src/realtime_assistant/types.rs`
- Create: `frontend/src-tauri/src/realtime_assistant/window.rs`
- Create: `frontend/src-tauri/src/realtime_assistant/prompt.rs`
- Test: inline unit tests in `window.rs` and `prompt.rs`

- [ ] **Step 1: Write failing tests for rolling window and JSON cleanup**

Create `frontend/src-tauri/src/realtime_assistant/mod.rs`:

```rust
pub mod commands;
pub mod prompt;
pub mod service;
pub mod types;
pub mod window;
```

Create `frontend/src-tauri/src/realtime_assistant/types.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeAssistantConfig {
    pub enabled: bool,
    pub system_prompt: String,
    pub window_seconds: u32,
    pub min_interval_ms: u64,
    pub min_new_chars: usize,
    pub max_items: usize,
    pub include_partial: bool,
    pub minimum_confidence: f32,
    pub allow_cloud_providers: bool,
}

impl Default for RealtimeAssistantConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            system_prompt: "Ты realtime assistant на звонке. Дай короткие полезные рекомендации пользователю: что уточнить, какие риски заметить, какой следующий шаг предложить. Отвечай только JSON по заданной схеме.".to_string(),
            window_seconds: 120,
            min_interval_ms: 8_000,
            min_new_chars: 160,
            max_items: 4,
            include_partial: false,
            minimum_confidence: 0.50,
            allow_cloud_providers: false,
        }
    }
}

impl RealtimeAssistantConfig {
    pub fn normalized(mut self) -> Self {
        self.window_seconds = self.window_seconds.clamp(30, 300);
        self.min_interval_ms = self.min_interval_ms.clamp(3_000, 30_000);
        self.min_new_chars = self.min_new_chars.clamp(40, 2_000);
        self.max_items = self.max_items.clamp(1, 8);
        self.minimum_confidence = self.minimum_confidence.clamp(0.0, 1.0);
        if self.system_prompt.trim().is_empty() {
            self.system_prompt = Self::default().system_prompt;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTranscriptSegment {
    pub id: String,
    pub text: String,
    pub sequence_id: Option<u64>,
    pub is_partial: bool,
    pub confidence: Option<f32>,
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeAssistantEvaluateRequest {
    pub session_id: String,
    pub request_id: String,
    pub transcript_segments: Vec<AssistantTranscriptSegment>,
    pub config: Option<RealtimeAssistantConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeAssistantItem {
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeAssistantEvaluation {
    pub request_id: String,
    pub recommendations: Vec<RealtimeAssistantItem>,
    pub questions_to_ask: Vec<RealtimeAssistantItem>,
    pub risks: Vec<RealtimeAssistantItem>,
    pub next_actions: Vec<RealtimeAssistantItem>,
    pub confidence: f32,
    pub model_provider: String,
    pub model_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeAssistantSession {
    pub session_id: String,
    pub config: RealtimeAssistantConfig,
}
```

Create `frontend/src-tauri/src/realtime_assistant/window.rs` with tests:

```rust
use super::types::{AssistantTranscriptSegment, RealtimeAssistantConfig};

pub fn select_window(
    segments: &[AssistantTranscriptSegment],
    config: &RealtimeAssistantConfig,
) -> Vec<AssistantTranscriptSegment> {
    let latest_end = segments
        .iter()
        .filter_map(|segment| segment.audio_end_time.or(segment.audio_start_time))
        .fold(0.0_f64, f64::max);
    let min_start = (latest_end - config.window_seconds as f64).max(0.0);

    let mut selected: Vec<AssistantTranscriptSegment> = segments
        .iter()
        .filter(|segment| config.include_partial || !segment.is_partial)
        .filter(|segment| {
            segment
                .confidence
                .map(|confidence| confidence >= config.minimum_confidence)
                .unwrap_or(true)
        })
        .filter(|segment| {
            segment
                .audio_end_time
                .or(segment.audio_start_time)
                .map(|time| time >= min_start)
                .unwrap_or(true)
        })
        .filter(|segment| !segment.text.trim().is_empty())
        .cloned()
        .collect();

    selected.sort_by(|a, b| {
        let a_time = a.audio_start_time.unwrap_or(0.0);
        let b_time = b.audio_start_time.unwrap_or(0.0);
        a_time
            .partial_cmp(&b_time)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.sequence_id.unwrap_or(0).cmp(&b.sequence_id.unwrap_or(0)))
    });

    selected
}

pub fn format_window_for_prompt(segments: &[AssistantTranscriptSegment]) -> String {
    segments
        .iter()
        .map(|segment| {
            let start = segment.audio_start_time.unwrap_or(0.0);
            format!(
                "[{}] {}",
                format_mm_ss(start),
                segment.text.trim().replace('\n', " ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_mm_ss(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(id: &str, text: &str, start: f64, confidence: f32, partial: bool) -> AssistantTranscriptSegment {
        AssistantTranscriptSegment {
            id: id.to_string(),
            text: text.to_string(),
            sequence_id: id.parse::<u64>().ok(),
            is_partial: partial,
            confidence: Some(confidence),
            audio_start_time: Some(start),
            audio_end_time: Some(start + 4.0),
        }
    }

    #[test]
    fn select_window_filters_old_partial_and_low_confidence_segments() {
        let config = RealtimeAssistantConfig {
            window_seconds: 60,
            include_partial: false,
            minimum_confidence: 0.6,
            ..RealtimeAssistantConfig::default()
        };
        let segments = vec![
            segment("1", "old", 10.0, 0.9, false),
            segment("2", "partial", 72.0, 0.9, true),
            segment("3", "low confidence", 76.0, 0.2, false),
            segment("4", "client doubts price", 80.0, 0.9, false),
        ];

        let selected = select_window(&segments, &config);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].text, "client doubts price");
    }

    #[test]
    fn format_window_uses_recording_relative_timestamps() {
        let segments = vec![segment("1", "Client doubts price", 125.2, 0.9, false)];

        let formatted = format_window_for_prompt(&segments);

        assert_eq!(formatted, "[02:05] Client doubts price");
    }
}
```

Create `frontend/src-tauri/src/realtime_assistant/prompt.rs` with tests:

```rust
use super::types::RealtimeAssistantConfig;

pub fn build_system_prompt(config: &RealtimeAssistantConfig) -> String {
    format!(
        "{}\n\nОтвет должен быть валидным JSON без markdown fences. Schema: {{\"recommendations\":[{{\"title\":\"Уточнить бюджет\",\"detail\":\"Спросить диапазон бюджета и критерии приемлемой цены\"}}],\"questionsToAsk\":[{{\"title\":\"Критерии успеха\",\"detail\":\"Уточнить, какой результат клиент считает успешным\"}}],\"risks\":[{{\"title\":\"Сроки\",\"detail\":\"Проверить, есть ли жёсткий deadline\"}}],\"nextActions\":[{{\"title\":\"Зафиксировать next step\",\"detail\":\"Предложить следующий созвон или письменное резюме\"}}],\"confidence\":0.7}}. Максимум {} items в каждом массиве.",
        config.system_prompt.trim(),
        config.max_items
    )
}

pub fn build_user_prompt(formatted_transcript_window: &str) -> String {
    format!(
        "Окно live transcript:\n<transcript>\n{}\n</transcript>\n\nВерни рекомендации только по новым и важным сигналам из окна.",
        formatted_transcript_window.trim()
    )
}

pub fn clean_json_response(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim();
    without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_system_prompt_includes_json_contract() {
        let prompt = build_system_prompt(&RealtimeAssistantConfig::default());

        assert!(prompt.contains("валидным JSON"));
        assert!(prompt.contains("questionsToAsk"));
    }

    #[test]
    fn clean_json_response_removes_markdown_fence() {
        let raw = "```json\n{\"confidence\":0.7}\n```";

        assert_eq!(clean_json_response(raw), "{\"confidence\":0.7}");
    }
}
```

- [ ] **Step 2: Run tests to verify failure from missing module registration**

Run:

```bash
cargo test -p meetily realtime_assistant --lib
```

Expected: FAIL with unresolved module errors until `lib.rs` exposes the module in Task 3.

- [ ] **Step 3: Commit pure helper files**

```bash
git add frontend/src-tauri/src/realtime_assistant
git commit -m "feat: add realtime assistant core types"
```

---

### Task 2: Backend Service, Config Store, Privacy Gate, LLM Evaluation

**Files:**
- Create: `frontend/src-tauri/src/realtime_assistant/service.rs`
- Create: `frontend/src-tauri/src/realtime_assistant/commands.rs`
- Modify: `frontend/src-tauri/src/realtime_assistant/types.rs`
- Test: inline unit tests in `service.rs`

- [ ] **Step 1: Write failing tests for privacy gate and response parsing**

Create `frontend/src-tauri/src/realtime_assistant/service.rs`:

```rust
use super::prompt::{build_system_prompt, build_user_prompt, clean_json_response};
use super::types::{
    RealtimeAssistantConfig, RealtimeAssistantEvaluateRequest, RealtimeAssistantEvaluation,
};
use super::window::{format_window_for_prompt, select_window};
use crate::database::repositories::setting::SettingsRepository;
use crate::state::AppState;
use crate::summary::llm_client::{generate_summary, LLMProvider};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;
use tokio_util::sync::CancellationToken;

const STORE_FILE: &str = "realtime-assistant.json";
const STORE_KEY_CONFIG: &str = "config";
const EVALUATION_TIMEOUT_SECONDS: u64 = 25;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmEvaluationPayload {
    #[serde(default)]
    recommendations: Vec<super::types::RealtimeAssistantItem>,
    #[serde(default)]
    questions_to_ask: Vec<super::types::RealtimeAssistantItem>,
    #[serde(default)]
    risks: Vec<super::types::RealtimeAssistantItem>,
    #[serde(default)]
    next_actions: Vec<super::types::RealtimeAssistantItem>,
    #[serde(default)]
    confidence: f32,
}

pub async fn load_config<R: Runtime>(app: &AppHandle<R>) -> Result<RealtimeAssistantConfig, String> {
    let store = app
        .store(STORE_FILE)
        .map_err(|error| format!("Failed to open realtime assistant store: {}", error))?;

    let config = match store.get(STORE_KEY_CONFIG) {
        Some(value) => serde_json::from_value::<RealtimeAssistantConfig>(value.clone())
            .map_err(|error| format!("Failed to parse realtime assistant config: {}", error))?,
        None => RealtimeAssistantConfig::default(),
    };

    Ok(config.normalized())
}

pub async fn save_config<R: Runtime>(
    app: &AppHandle<R>,
    config: RealtimeAssistantConfig,
) -> Result<RealtimeAssistantConfig, String> {
    let normalized = config.normalized();
    let store = app
        .store(STORE_FILE)
        .map_err(|error| format!("Failed to open realtime assistant store: {}", error))?;
    let value = serde_json::to_value(&normalized)
        .map_err(|error| format!("Failed to serialize realtime assistant config: {}", error))?;
    store.set(STORE_KEY_CONFIG, value);
    store
        .save()
        .map_err(|error| format!("Failed to save realtime assistant config: {}", error))?;
    Ok(normalized)
}

pub async fn evaluate_window<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    request: RealtimeAssistantEvaluateRequest,
) -> Result<RealtimeAssistantEvaluation, String> {
    let config = request
        .config
        .clone()
        .unwrap_or(load_config(&app).await?)
        .normalized();

    if !config.enabled {
        return Err("Realtime assistant is disabled".to_string());
    }

    let selected = select_window(&request.transcript_segments, &config);
    if selected.is_empty() {
        return Err("No transcript segments available for realtime assistant".to_string());
    }

    let formatted_window = format_window_for_prompt(&selected);
    if formatted_window.chars().count() < config.min_new_chars {
        return Err("Transcript delta is too small for realtime assistant evaluation".to_string());
    }

    let pool = state.db_manager.pool();
    let model_config = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|error| format!("Failed to load model config: {}", error))?
        .ok_or_else(|| "Summary model config is missing".to_string())?;

    let provider = LLMProvider::from_str(&model_config.provider)?;
    let (api_key, ollama_endpoint, custom_endpoint, max_tokens, temperature, top_p) =
        resolve_provider_settings(pool, &provider, &model_config.provider, model_config.ollama_endpoint.clone()).await?;

    enforce_privacy_gate(&provider, custom_endpoint.as_deref(), config.allow_cloud_providers)?;

    let system_prompt = build_system_prompt(&config);
    let user_prompt = build_user_prompt(&formatted_window);
    let client = reqwest::Client::new();
    let cancellation_token = CancellationToken::new();
    let app_data_dir: Option<PathBuf> = app.path().app_data_dir().ok();

    let generation = generate_summary(
        &client,
        &provider,
        &model_config.model,
        &api_key,
        &system_prompt,
        &user_prompt,
        ollama_endpoint.as_deref(),
        custom_endpoint.as_deref(),
        max_tokens.or(Some(900)),
        temperature.or(Some(0.2)),
        top_p,
        app_data_dir.as_ref(),
        Some(&cancellation_token),
    );

    let raw = tokio::time::timeout(Duration::from_secs(EVALUATION_TIMEOUT_SECONDS), generation)
        .await
        .map_err(|_| "Realtime assistant LLM request timed out".to_string())??;

    parse_evaluation(&request.request_id, &model_config.provider, &model_config.model, &raw, config.max_items)
}

async fn resolve_provider_settings(
    pool: &sqlx::SqlitePool,
    provider: &LLMProvider,
    provider_name: &str,
    ollama_endpoint_from_model_config: Option<String>,
) -> Result<(String, Option<String>, Option<String>, Option<u32>, Option<f32>, Option<f32>), String> {
    match provider {
        LLMProvider::Ollama => Ok((String::new(), ollama_endpoint_from_model_config, None, None, None, None)),
        LLMProvider::BuiltInAI => Ok((String::new(), None, None, None, None, None)),
        LLMProvider::CustomOpenAI => {
            let custom = SettingsRepository::get_custom_openai_config(pool)
                .await
                .map_err(|error| format!("Failed to load custom OpenAI config: {}", error))?
                .ok_or_else(|| "Custom OpenAI config is missing".to_string())?;
            Ok((
                custom.api_key.unwrap_or_default(),
                None,
                Some(custom.endpoint),
                custom.max_tokens.map(|value| value as u32),
                custom.temperature,
                custom.top_p,
            ))
        }
        _ => {
            let api_key = SettingsRepository::get_api_key(pool, provider_name)
                .await
                .map_err(|error| format!("Failed to load API key for {}: {}", provider_name, error))?
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| format!("API key is missing for {}", provider_name))?;
            Ok((api_key, None, None, None, None, None))
        }
    }
}

fn enforce_privacy_gate(
    provider: &LLMProvider,
    custom_endpoint: Option<&str>,
    allow_cloud_providers: bool,
) -> Result<(), String> {
    if allow_cloud_providers {
        return Ok(());
    }

    match provider {
        LLMProvider::Ollama | LLMProvider::BuiltInAI => Ok(()),
        LLMProvider::CustomOpenAI if custom_endpoint.map(is_local_endpoint).unwrap_or(false) => Ok(()),
        _ => Err("Realtime assistant is in local-only mode. Enable cloud providers in Assistant settings to send transcript windows to external APIs.".to_string()),
    }
}

fn is_local_endpoint(endpoint: &str) -> bool {
    let normalized = endpoint.trim().to_lowercase();
    normalized.starts_with("http://localhost")
        || normalized.starts_with("http://127.0.0.1")
        || normalized.starts_with("http://[::1]")
}

fn parse_evaluation(
    request_id: &str,
    model_provider: &str,
    model_name: &str,
    raw: &str,
    max_items: usize,
) -> Result<RealtimeAssistantEvaluation, String> {
    let cleaned = clean_json_response(raw);
    let mut payload: LlmEvaluationPayload = serde_json::from_str(&cleaned)
        .map_err(|error| format!("Failed to parse realtime assistant JSON response: {}", error))?;
    payload.recommendations.truncate(max_items);
    payload.questions_to_ask.truncate(max_items);
    payload.risks.truncate(max_items);
    payload.next_actions.truncate(max_items);

    Ok(RealtimeAssistantEvaluation {
        request_id: request_id.to_string(),
        recommendations: payload.recommendations,
        questions_to_ask: payload.questions_to_ask,
        risks: payload.risks,
        next_actions: payload.next_actions,
        confidence: payload.confidence.clamp(0.0, 1.0),
        model_provider: model_provider.to_string(),
        model_name: model_name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::llm_client::LLMProvider;

    #[test]
    fn privacy_gate_allows_local_providers() {
        assert!(enforce_privacy_gate(&LLMProvider::Ollama, None, false).is_ok());
        assert!(enforce_privacy_gate(&LLMProvider::BuiltInAI, None, false).is_ok());
        assert!(enforce_privacy_gate(&LLMProvider::CustomOpenAI, Some("http://localhost:8000/v1"), false).is_ok());
    }

    #[test]
    fn privacy_gate_blocks_cloud_provider_by_default() {
        let result = enforce_privacy_gate(&LLMProvider::OpenRouter, None, false);

        assert!(result.unwrap_err().contains("local-only mode"));
    }

    #[test]
    fn parse_evaluation_truncates_items_and_clamps_confidence() {
        let raw = r#"{
            "recommendations":[{"title":"A","detail":"1"},{"title":"B","detail":"2"}],
            "questionsToAsk":[{"title":"Q","detail":"Ask"}],
            "risks":[],
            "nextActions":[],
            "confidence":2.5
        }"#;

        let parsed = parse_evaluation("req-1", "ollama", "llama3", raw, 1).unwrap();

        assert_eq!(parsed.recommendations.len(), 1);
        assert_eq!(parsed.confidence, 1.0);
        assert_eq!(parsed.request_id, "req-1");
    }
}
```

- [ ] **Step 2: Add Tauri command wrappers**

Create `frontend/src-tauri/src/realtime_assistant/commands.rs`:

```rust
use super::service;
use super::types::{
    RealtimeAssistantConfig, RealtimeAssistantEvaluateRequest, RealtimeAssistantEvaluation,
    RealtimeAssistantSession,
};
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

#[tauri::command]
pub async fn assistant_get_config<R: Runtime>(
    app: AppHandle<R>,
) -> Result<RealtimeAssistantConfig, String> {
    service::load_config(&app).await
}

#[tauri::command]
pub async fn assistant_save_config<R: Runtime>(
    app: AppHandle<R>,
    config: RealtimeAssistantConfig,
) -> Result<RealtimeAssistantConfig, String> {
    service::save_config(&app, config).await
}

#[tauri::command]
pub async fn assistant_start_session<R: Runtime>(
    app: AppHandle<R>,
) -> Result<RealtimeAssistantSession, String> {
    let config = service::load_config(&app).await?;
    Ok(RealtimeAssistantSession {
        session_id: Uuid::new_v4().to_string(),
        config,
    })
}

#[tauri::command]
pub async fn assistant_stop_session<R: Runtime>(
    _app: AppHandle<R>,
    _session_id: String,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn assistant_evaluate_window<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    request: RealtimeAssistantEvaluateRequest,
) -> Result<RealtimeAssistantEvaluation, String> {
    let evaluation = service::evaluate_window(app.clone(), state, request).await?;
    let _ = app.emit("assistant-recommendations-update", &evaluation);
    Ok(evaluation)
}
```

- [ ] **Step 3: Run targeted tests**

Run:

```bash
cargo test -p meetily realtime_assistant --lib
```

Expected: tests compile and pass after Task 3 registers module in `lib.rs`. If this step runs before Task 3, unresolved module failure is expected.

- [ ] **Step 4: Commit backend service**

```bash
git add frontend/src-tauri/src/realtime_assistant
git commit -m "feat: add realtime assistant backend service"
```

---

### Task 3: Register Rust Module And Commands

**Files:**
- Modify: `frontend/src-tauri/src/lib.rs`
- Test: `cargo test -p meetily realtime_assistant --lib`

- [ ] **Step 1: Add module export**

Modify `frontend/src-tauri/src/lib.rs` near existing module declarations:

```rust
pub mod realtime_assistant;
```

Place it with the other app modules, for example after:

```rust
pub mod parakeet_engine;
pub mod realtime_assistant;
pub mod state;
```

- [ ] **Step 2: Register commands in invoke handler**

Modify the `tauri::generate_handler!` command list in `frontend/src-tauri/src/lib.rs` near summary/template commands:

```rust
            // Realtime assistant commands
            realtime_assistant::commands::assistant_get_config,
            realtime_assistant::commands::assistant_save_config,
            realtime_assistant::commands::assistant_start_session,
            realtime_assistant::commands::assistant_stop_session,
            realtime_assistant::commands::assistant_evaluate_window,
```

- [ ] **Step 3: Run tests**

Run:

```bash
cargo test -p meetily realtime_assistant --lib
cargo check -p meetily
```

Expected: realtime assistant tests pass; `cargo check` passes.

- [ ] **Step 4: Commit command registration**

```bash
git add frontend/src-tauri/src/lib.rs frontend/src-tauri/src/realtime_assistant
git commit -m "feat: register realtime assistant commands"
```

---

### Task 4: Frontend Types And Service

**Files:**
- Modify: `frontend/src/types/index.ts`
- Create: `frontend/src/services/realtimeAssistantService.ts`
- Test: TypeScript compile through Next build/lint

- [ ] **Step 1: Add TypeScript DTOs**

Append to `frontend/src/types/index.ts`:

```typescript
export interface RealtimeAssistantConfig {
  enabled: boolean;
  systemPrompt: string;
  windowSeconds: number;
  minIntervalMs: number;
  minNewChars: number;
  maxItems: number;
  includePartial: boolean;
  minimumConfidence: number;
  allowCloudProviders: boolean;
}

export interface AssistantTranscriptSegment {
  id: string;
  text: string;
  sequenceId?: number;
  isPartial: boolean;
  confidence?: number;
  audioStartTime?: number;
  audioEndTime?: number;
}

export interface RealtimeAssistantEvaluateRequest {
  sessionId: string;
  requestId: string;
  transcriptSegments: AssistantTranscriptSegment[];
  config?: RealtimeAssistantConfig;
}

export interface RealtimeAssistantItem {
  title: string;
  detail: string;
}

export interface RealtimeAssistantEvaluation {
  requestId: string;
  recommendations: RealtimeAssistantItem[];
  questionsToAsk: RealtimeAssistantItem[];
  risks: RealtimeAssistantItem[];
  nextActions: RealtimeAssistantItem[];
  confidence: number;
  modelProvider: string;
  modelName: string;
}

export interface RealtimeAssistantSession {
  sessionId: string;
  config: RealtimeAssistantConfig;
}
```

- [ ] **Step 2: Add service wrapper**

Create `frontend/src/services/realtimeAssistantService.ts`:

```typescript
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import {
  RealtimeAssistantConfig,
  RealtimeAssistantEvaluateRequest,
  RealtimeAssistantEvaluation,
  RealtimeAssistantSession,
} from '@/types';

class RealtimeAssistantService {
  getConfig(): Promise<RealtimeAssistantConfig> {
    return invoke<RealtimeAssistantConfig>('assistant_get_config');
  }

  saveConfig(config: RealtimeAssistantConfig): Promise<RealtimeAssistantConfig> {
    return invoke<RealtimeAssistantConfig>('assistant_save_config', { config });
  }

  startSession(): Promise<RealtimeAssistantSession> {
    return invoke<RealtimeAssistantSession>('assistant_start_session');
  }

  stopSession(sessionId: string): Promise<void> {
    return invoke<void>('assistant_stop_session', { sessionId });
  }

  evaluateWindow(request: RealtimeAssistantEvaluateRequest): Promise<RealtimeAssistantEvaluation> {
    return invoke<RealtimeAssistantEvaluation>('assistant_evaluate_window', { request });
  }

  onRecommendationsUpdate(
    callback: (evaluation: RealtimeAssistantEvaluation) => void
  ): Promise<UnlistenFn> {
    return listen<RealtimeAssistantEvaluation>('assistant-recommendations-update', (event) => {
      callback(event.payload);
    });
  }
}

export const realtimeAssistantService = new RealtimeAssistantService();
```

- [ ] **Step 3: Run TypeScript checks through existing scripts**

Run:

```bash
cd frontend
pnpm run lint
pnpm run build
```

Expected: no TypeScript import/type errors from the new service.

- [ ] **Step 4: Commit frontend service**

```bash
git add frontend/src/types/index.ts frontend/src/services/realtimeAssistantService.ts
git commit -m "feat: add realtime assistant frontend service"
```

---

### Task 5: Frontend Hook With Throttle And In-Flight Guard

**Files:**
- Create: `frontend/src/hooks/useRealtimeAssistant.ts`
- Modify: `frontend/src/app/page.tsx`
- Test: lint/build

- [ ] **Step 1: Create hook**

Create `frontend/src/hooks/useRealtimeAssistant.ts`:

```typescript
'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { realtimeAssistantService } from '@/services/realtimeAssistantService';
import {
  AssistantTranscriptSegment,
  RealtimeAssistantConfig,
  RealtimeAssistantEvaluation,
  Transcript,
} from '@/types';
import { useRecordingState } from '@/contexts/RecordingStateContext';

interface UseRealtimeAssistantResult {
  config: RealtimeAssistantConfig | null;
  latestEvaluation: RealtimeAssistantEvaluation | null;
  isLoadingConfig: boolean;
  isEvaluating: boolean;
  error: string | null;
  saveConfig: (config: RealtimeAssistantConfig) => Promise<void>;
}

function transcriptToAssistantSegment(transcript: Transcript): AssistantTranscriptSegment {
  return {
    id: transcript.id,
    text: transcript.text,
    sequenceId: transcript.sequence_id,
    isPartial: transcript.is_partial ?? false,
    confidence: transcript.confidence,
    audioStartTime: transcript.audio_start_time,
    audioEndTime: transcript.audio_end_time,
  };
}

function countChars(transcripts: Transcript[]): number {
  return transcripts.reduce((sum, transcript) => sum + transcript.text.trim().length, 0);
}

export function useRealtimeAssistant(transcripts: Transcript[]): UseRealtimeAssistantResult {
  const recordingState = useRecordingState();
  const [config, setConfig] = useState<RealtimeAssistantConfig | null>(null);
  const [latestEvaluation, setLatestEvaluation] = useState<RealtimeAssistantEvaluation | null>(null);
  const [isLoadingConfig, setIsLoadingConfig] = useState(true);
  const [isEvaluating, setIsEvaluating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const sessionIdRef = useRef<string | null>(null);
  const inFlightRef = useRef(false);
  const lastEvaluateAtRef = useRef(0);
  const lastEvaluatedCharsRef = useRef(0);
  const latestRequestIdRef = useRef<string | null>(null);

  useEffect(() => {
    let active = true;
    realtimeAssistantService
      .getConfig()
      .then((loadedConfig) => {
        if (active) {
          setConfig(loadedConfig);
          setError(null);
        }
      })
      .catch((err) => {
        if (active) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (active) setIsLoadingConfig(false);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!config?.enabled || !recordingState.isRecording) {
      if (sessionIdRef.current) {
        const sessionId = sessionIdRef.current;
        sessionIdRef.current = null;
        realtimeAssistantService.stopSession(sessionId).catch(() => undefined);
      }
      return;
    }

    if (!sessionIdRef.current) {
      realtimeAssistantService
        .startSession()
        .then((session) => {
          sessionIdRef.current = session.sessionId;
          setConfig(session.config);
        })
        .catch((err) => setError(err instanceof Error ? err.message : String(err)));
    }
  }, [config?.enabled, recordingState.isRecording]);

  const visibleSegments = useMemo(() => transcripts.map(transcriptToAssistantSegment), [transcripts]);

  useEffect(() => {
    if (!config?.enabled || !recordingState.isRecording || !sessionIdRef.current) return;
    if (visibleSegments.length === 0) return;
    if (inFlightRef.current) return;

    const now = Date.now();
    if (now - lastEvaluateAtRef.current < config.minIntervalMs) return;

    const currentChars = countChars(transcripts);
    if (currentChars - lastEvaluatedCharsRef.current < config.minNewChars) return;

    const requestId = `${now}-${visibleSegments.length}`;
    latestRequestIdRef.current = requestId;
    inFlightRef.current = true;
    setIsEvaluating(true);

    realtimeAssistantService
      .evaluateWindow({
        sessionId: sessionIdRef.current,
        requestId,
        transcriptSegments: visibleSegments,
        config,
      })
      .then((evaluation) => {
        if (latestRequestIdRef.current === evaluation.requestId) {
          setLatestEvaluation(evaluation);
          setError(null);
          lastEvaluateAtRef.current = now;
          lastEvaluatedCharsRef.current = currentChars;
        }
      })
      .catch((err) => {
        const message = err instanceof Error ? err.message : String(err);
        if (!message.includes('delta is too small')) {
          setError(message);
        }
      })
      .finally(() => {
        inFlightRef.current = false;
        setIsEvaluating(false);
      });
  }, [config, recordingState.isRecording, transcripts, visibleSegments]);

  const saveConfig = useCallback(async (nextConfig: RealtimeAssistantConfig) => {
    const saved = await realtimeAssistantService.saveConfig(nextConfig);
    setConfig(saved);
  }, []);

  return {
    config,
    latestEvaluation,
    isLoadingConfig,
    isEvaluating,
    error,
    saveConfig,
  };
}
```

- [ ] **Step 2: Wire hook in page without showing panel yet**

Modify imports in `frontend/src/app/page.tsx`:

```typescript
import { useRealtimeAssistant } from '@/hooks/useRealtimeAssistant';
```

Modify transcript context extraction:

```typescript
const { meetingTitle, transcripts } = useTranscripts();
```

Add hook call after recording state is available:

```typescript
const realtimeAssistant = useRealtimeAssistant(transcripts);
```

- [ ] **Step 3: Run checks**

Run:

```bash
cd frontend
pnpm run lint
pnpm run build
```

Expected: hook imports and page compile.

- [ ] **Step 4: Commit hook**

```bash
git add frontend/src/hooks/useRealtimeAssistant.ts frontend/src/app/page.tsx
git commit -m "feat: evaluate transcripts with realtime assistant hook"
```

---

### Task 6: Realtime Assistant Panel On Main Recording Screen

**Files:**
- Create: `frontend/src/components/RealtimeAssistantPanel.tsx`
- Modify: `frontend/src/app/page.tsx`
- Test: lint/build, manual layout check

- [ ] **Step 1: Create panel component**

Create `frontend/src/components/RealtimeAssistantPanel.tsx`:

```typescript
'use client';

import { AlertTriangle, CheckCircle2, HelpCircle, Lightbulb, Loader2 } from 'lucide-react';
import { RealtimeAssistantEvaluation, RealtimeAssistantItem } from '@/types';

interface RealtimeAssistantPanelProps {
  evaluation: RealtimeAssistantEvaluation | null;
  enabled: boolean;
  isEvaluating: boolean;
  error: string | null;
}

function ItemList({
  title,
  icon,
  items,
}: {
  title: string;
  icon: React.ReactNode;
  items: RealtimeAssistantItem[];
}) {
  if (items.length === 0) return null;

  return (
    <section className="space-y-2">
      <div className="flex items-center gap-2 text-sm font-semibold text-gray-800">
        {icon}
        <span>{title}</span>
      </div>
      <div className="space-y-2">
        {items.map((item, index) => (
          <article key={`${title}-${index}`} className="rounded-lg border border-gray-200 bg-white p-3">
            <h4 className="text-sm font-semibold text-gray-900">{item.title}</h4>
            <p className="mt-1 text-sm leading-5 text-gray-600">{item.detail}</p>
          </article>
        ))}
      </div>
    </section>
  );
}

export function RealtimeAssistantPanel({
  evaluation,
  enabled,
  isEvaluating,
  error,
}: RealtimeAssistantPanelProps) {
  if (!enabled) return null;

  return (
    <aside className="hidden xl:flex w-[360px] shrink-0 flex-col border-l border-gray-200 bg-gray-50">
      <div className="sticky top-0 z-10 border-b border-gray-200 bg-gray-50 px-4 py-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-semibold text-gray-900">Live assistant</h2>
            <p className="text-xs text-gray-500">
              {evaluation ? `${evaluation.modelProvider} · ${evaluation.modelName}` : 'Waiting for transcript'}
            </p>
          </div>
          {isEvaluating && <Loader2 className="h-4 w-4 animate-spin text-blue-600" />}
        </div>
      </div>

      <div className="flex-1 space-y-4 overflow-y-auto p-4 pb-28">
        {error && (
          <div className="rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700">
            {error}
          </div>
        )}

        {!evaluation && !error && (
          <div className="rounded-lg border border-gray-200 bg-white p-4 text-sm text-gray-600">
            Assistant появится после первых осмысленных фраз в transcript.
          </div>
        )}

        {evaluation && (
          <>
            <ItemList
              title="Recommendations"
              icon={<Lightbulb className="h-4 w-4 text-blue-600" />}
              items={evaluation.recommendations}
            />
            <ItemList
              title="Questions"
              icon={<HelpCircle className="h-4 w-4 text-emerald-600" />}
              items={evaluation.questionsToAsk}
            />
            <ItemList
              title="Risks"
              icon={<AlertTriangle className="h-4 w-4 text-amber-600" />}
              items={evaluation.risks}
            />
            <ItemList
              title="Next actions"
              icon={<CheckCircle2 className="h-4 w-4 text-violet-600" />}
              items={evaluation.nextActions}
            />
          </>
        )}
      </div>
    </aside>
  );
}
```

- [ ] **Step 2: Add panel to home layout**

Modify imports in `frontend/src/app/page.tsx`:

```typescript
import { RealtimeAssistantPanel } from '@/components/RealtimeAssistantPanel';
```

Render it after `TranscriptPanel`:

```tsx
        <TranscriptPanel
          isProcessingStop={isProcessingStop}
          isStopping={isStopping}
          showModal={showModal}
        />

        <RealtimeAssistantPanel
          evaluation={realtimeAssistant.latestEvaluation}
          enabled={realtimeAssistant.config?.enabled ?? false}
          isEvaluating={realtimeAssistant.isEvaluating}
          error={realtimeAssistant.error}
        />
```

- [ ] **Step 3: Run checks**

Run:

```bash
cd frontend
pnpm run lint
pnpm run build
```

Expected: panel compiles; layout remains a single flex row.

- [ ] **Step 4: Manual layout check**

Run:

```bash
cd frontend
pnpm run tauri:dev
```

Expected:
- Main transcript remains readable.
- On desktop `xl`, assistant panel appears at the right only when enabled.
- On smaller widths, panel is hidden and transcript takes full width.

- [ ] **Step 5: Commit panel**

```bash
git add frontend/src/components/RealtimeAssistantPanel.tsx frontend/src/app/page.tsx
git commit -m "feat: show realtime assistant panel"
```

---

### Task 7: Assistant Settings Tab

**Files:**
- Create: `frontend/src/components/RealtimeAssistantSettings.tsx`
- Modify: `frontend/src/app/settings/page.tsx`
- Test: lint/build, manual settings smoke

- [ ] **Step 1: Create settings component**

Create `frontend/src/components/RealtimeAssistantSettings.tsx`:

```typescript
'use client';

import { useEffect, useState } from 'react';
import { Bot, Cloud, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { realtimeAssistantService } from '@/services/realtimeAssistantService';
import { RealtimeAssistantConfig } from '@/types';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';

export function RealtimeAssistantSettings() {
  const [config, setConfig] = useState<RealtimeAssistantConfig | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    realtimeAssistantService
      .getConfig()
      .then(setConfig)
      .catch((error) => toast.error(error instanceof Error ? error.message : String(error)))
      .finally(() => setIsLoading(false));
  }, []);

  const updateConfig = <K extends keyof RealtimeAssistantConfig>(key: K, value: RealtimeAssistantConfig[K]) => {
    setConfig((current) => current ? Object.assign({}, current, { [key]: value }) : current);
  };

  const handleSave = async () => {
    if (!config) return;
    setIsSaving(true);
    try {
      const saved = await realtimeAssistantService.saveConfig(config);
      setConfig(saved);
      toast.success('Assistant settings saved');
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setIsSaving(false);
    }
  };

  if (isLoading || !config) {
    return (
      <div className="flex items-center gap-2 p-6 text-sm text-gray-600">
        <Loader2 className="h-4 w-4 animate-spin" />
        Loading assistant settings
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
        <div className="flex items-center justify-between gap-6">
          <div>
            <div className="flex items-center gap-2">
              <Bot className="h-5 w-5 text-gray-700" />
              <h3 className="text-lg font-semibold text-gray-900">Realtime assistant</h3>
            </div>
            <p className="mt-2 text-sm text-gray-600">
              Runs during active recording and reads a rolling transcript window.
            </p>
          </div>
          <Switch checked={config.enabled} onCheckedChange={(checked) => updateConfig('enabled', checked)} />
        </div>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
        <Label htmlFor="assistant-system-prompt">System prompt</Label>
        <Textarea
          id="assistant-system-prompt"
          className="mt-3 min-h-[140px]"
          value={config.systemPrompt}
          onChange={(event) => updateConfig('systemPrompt', event.target.value)}
        />
      </div>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <label className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
          <span className="text-sm font-medium text-gray-900">Window seconds</span>
          <Input
            className="mt-2"
            type="number"
            min={30}
            max={300}
            value={config.windowSeconds}
            onChange={(event) => updateConfig('windowSeconds', Number(event.target.value))}
          />
        </label>

        <label className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
          <span className="text-sm font-medium text-gray-900">Min interval ms</span>
          <Input
            className="mt-2"
            type="number"
            min={3000}
            max={30000}
            step={1000}
            value={config.minIntervalMs}
            onChange={(event) => updateConfig('minIntervalMs', Number(event.target.value))}
          />
        </label>

        <label className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
          <span className="text-sm font-medium text-gray-900">Min new chars</span>
          <Input
            className="mt-2"
            type="number"
            min={40}
            max={2000}
            value={config.minNewChars}
            onChange={(event) => updateConfig('minNewChars', Number(event.target.value))}
          />
        </label>

        <label className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
          <span className="text-sm font-medium text-gray-900">Minimum confidence</span>
          <Input
            className="mt-2"
            type="number"
            min={0}
            max={1}
            step={0.05}
            value={config.minimumConfidence}
            onChange={(event) => updateConfig('minimumConfidence', Number(event.target.value))}
          />
        </label>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
        <div className="flex items-center justify-between gap-6">
          <div>
            <div className="flex items-center gap-2">
              <Cloud className="h-5 w-5 text-gray-700" />
              <h3 className="text-lg font-semibold text-gray-900">Allow cloud providers</h3>
            </div>
            <p className="mt-2 text-sm text-gray-600">
              Sends transcript windows to the selected summary provider when it is OpenAI, OpenRouter, Claude, Groq, or remote Custom OpenAI.
            </p>
          </div>
          <Switch checked={config.allowCloudProviders} onCheckedChange={(checked) => updateConfig('allowCloudProviders', checked)} />
        </div>
      </div>

      <div className="flex justify-end">
        <Button onClick={handleSave} disabled={isSaving}>
          {isSaving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          Save
        </Button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add Assistant settings tab**

Modify imports in `frontend/src/app/settings/page.tsx`:

```typescript
import { ArrowLeft, Settings2, Mic, Database as DatabaseIcon, SparkleIcon, FlaskConical, Bot } from 'lucide-react';
import { RealtimeAssistantSettings } from '@/components/RealtimeAssistantSettings';
```

Modify `TABS`:

```typescript
  { value: 'assistant', label: 'Assistant', icon: Bot },
```

Place it after `summaryModels`:

```typescript
  { value: 'summaryModels', label: 'Summary', icon: SparkleIcon },
  { value: 'assistant', label: 'Assistant', icon: Bot },
  { value: 'beta', label: 'Beta', icon: FlaskConical }
```

Add tab content:

```tsx
            <TabsContent value="assistant">
              <RealtimeAssistantSettings />
            </TabsContent>
```

- [ ] **Step 3: Run checks**

Run:

```bash
cd frontend
pnpm run lint
pnpm run build
```

Expected: settings page compiles; tab list remains readable in the `max-w-6xl` layout.

- [ ] **Step 4: Manual settings smoke**

Run:

```bash
cd frontend
pnpm run tauri:dev
```

Expected:
- Settings has `Assistant` tab.
- Toggle can be enabled and saved.
- App restart keeps assistant config because `realtime-assistant.json` is persisted by Tauri Store.

- [ ] **Step 5: Commit settings**

```bash
git add frontend/src/components/RealtimeAssistantSettings.tsx frontend/src/app/settings/page.tsx
git commit -m "feat: add realtime assistant settings"
```

---

### Task 8: End-To-End Fail-Fast Verification And Docs

**Files:**
- Modify: `docs/realtime-assistant-research.md`
- Test: cargo, lint/build, manual realtime smoke

- [ ] **Step 1: Run full local verification**

Run:

```bash
cargo test -p meetily realtime_assistant --lib
cargo check -p meetily
cd frontend
pnpm run lint
pnpm run build
```

Expected:
- Rust tests pass.
- Rust crate checks.
- Frontend lint/build pass.

- [ ] **Step 2: Run manual realtime smoke**

Run:

```bash
cd frontend
pnpm run tauri:dev
```

Manual flow:

1. Open Settings -> Summary and choose `ollama` or `builtin-ai`.
2. Open Settings -> Assistant, enable realtime assistant, keep cloud providers disabled, save.
3. Start recording.
4. Say: `Клиент сомневается по цене и срокам. Нужно уточнить бюджет, deadline и критерии успеха.`
5. Wait 8-25 seconds.

Expected:
- Transcript updates appear in the main transcript panel.
- Right-side `Live assistant` panel appears on desktop width.
- Panel shows at least one recommendation or question.
- Stopping recording leaves no visible loading state.

- [ ] **Step 3: Document observed behavior**

Append to `docs/realtime-assistant-research.md`:

```markdown
## MVP Implementation Notes

- Realtime assistant implemented as internal plugin-shaped module under `frontend/src-tauri/src/realtime_assistant/`.
- Config persists in Tauri Store file `realtime-assistant.json`.
- Default privacy gate allows local providers: `ollama`, `builtin-ai`, and localhost `custom-openai`.
- Cloud providers require `allowCloudProviders=true` in Assistant settings.
- Frontend uses `useRealtimeAssistant` with in-flight guard, min interval, and min transcript delta.
- Manual smoke phrase: `Клиент сомневается по цене и срокам. Нужно уточнить бюджет, deadline и критерии успеха.`
```

- [ ] **Step 4: Commit docs and verification notes**

```bash
git add docs/realtime-assistant-research.md
git commit -m "docs: record realtime assistant mvp verification"
```

---

## Architecture Context Map

- Stage: MVP.
- Source docs: `docs/realtime-assistant-research.md`, `CLAUDE.md`, Tauri docs for commands/events/plugins.
- Active paths: `frontend/src-tauri/src/realtime_assistant`, `frontend/src/hooks/useRealtimeAssistant.ts`, `frontend/src/components/RealtimeAssistantPanel.tsx`.
- Critical gates: transcript privacy, no duplicate LLM calls, no React-side API key handling, no cloud provider use without explicit setting.
- Baseline gates: Rust unit tests for pure logic, `cargo check`, `pnpm run lint`, `pnpm run build`, manual Tauri smoke.
- Known deviations: first MVP uses non-streaming LLM response and hides assistant panel on smaller screens.
- Verification surface: unit tests, compile checks, manual local recording smoke.
- Open decisions: whether to persist assistant outputs into meeting history; whether to add streaming cards; whether to later extract as Tauri plugin crate.

## Self-Review

Spec coverage:

- Realtime transcription input covered by Task 5 via existing `TranscriptContext` state.
- LLM recommendations covered by Task 2 and Task 5.
- System prompt config covered by Task 7.
- Plugin-shaped boundary covered by new Rust module/service/commands and TS service.
- Privacy boundary covered by local-only gate in Task 2 and settings in Task 7.
- Fail-fast verification covered by Task 8.

Placeholder scan:

- Placeholder scan passed.
- Plan contains no unspecified implementation instructions.
- All new files have concrete code blocks.

Type consistency:

- Rust uses `serde(rename_all = "camelCase")`.
- TypeScript DTO names match Rust serialized fields.
- Event name is consistently `assistant-recommendations-update`.
- Tauri command names are consistently `assistant_*`.
