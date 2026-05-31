# Realtime assistant поверх Meetily

Дата: 2026-06-01

## Цель

Добавить приватный realtime-copilot поверх локальной записи Meetily: приложение слушает звонок, получает live transcript chunks и по системному промпту выдаёт рекомендации пользователю во время разговора.

## Текущая опора в проекте

- Live transcript уже есть: Rust transcription worker эмитит Tauri event `transcript-update`.
- Frontend уже слушает этот event через `TranscriptService.onTranscriptUpdate`.
- UI уже держит transcript state в `TranscriptContext`.
- LLM providers уже есть в Rust summary layer: `openai`, `openrouter`, `ollama`, `custom-openai`, `claude`, `groq`, `builtin-ai`.
- Custom OpenAI-compatible endpoint уже поддержан и тестируется через settings UI.

Ключевые файлы:

- `frontend/src-tauri/src/audio/transcription/worker.rs` — источник `transcript-update`.
- `frontend/src/services/transcriptService.ts` — frontend wrapper для transcript events.
- `frontend/src/contexts/TranscriptContext.tsx` — буферизация и state live transcript.
- `frontend/src-tauri/src/summary/llm_client.rs` — общий OpenAI-compatible LLM client.
- `frontend/src-tauri/src/summary/service.rs` — извлечение provider config/API keys.
- `frontend/src/components/ModelSettingsModal.tsx` — существующий UI для OpenRouter/OpenAI/custom endpoint.

## Рекомендуемый MVP-слайс

1. Ввести новый модуль `frontend/src-tauri/src/realtime_assistant/`.
2. Добавить Tauri commands:
   - `assistant_get_config`
   - `assistant_save_config`
   - `assistant_start_session`
   - `assistant_stop_session`
   - `assistant_evaluate_window`
3. В `TranscriptContext` после добавления новых transcript chunks вызывать debounce/throttle обработчик.
4. Rolling window: последние 60-180 секунд transcript segments.
5. LLM call: переиспользовать `summary::llm_client::generate_summary` или вынести общий `chat_completion` helper.
6. Ответ модели вернуть как strict JSON:
   - `recommendations`
   - `questions_to_ask`
   - `risks`
   - `next_actions`
   - `confidence`
7. Эмитить frontend event `assistant-recommendations-update`.
8. Добавить правую панель live-рекомендаций на главном экране записи.

## Почему это лучше делать в Rust/Tauri layer

LLM config и API keys уже живут в Rust/SQLite settings. Если делать вызовы из React, придётся заново решать хранение ключей, provider routing, custom endpoint и cancellation. Rust layer уже умеет это для summary, значит realtime assistant должен переиспользовать этот контур.

## Ограничения и риски

- `summary::llm_client::generate_summary` сейчас настроен под non-streaming ответ. Для MVP этого достаточно. Streaming ответа можно добавить отдельным слайсом.
- Частые LLM calls быстро станут дорогими. Нужен throttle: например, один call раз в 5-10 секунд или только после meaningful transcript delta.
- Нужно дедуплицировать рекомендации, иначе UI будет шумным.
- Нужно не слать partial/низкоуверенные chunks в LLM или помечать их отдельно.
- Для полной приватности использовать `ollama` или `builtin-ai`. OpenRouter/OpenAI отправляют transcript наружу.
- В текущей машине `pnpm` отсутствует в shell, поэтому перед implementation verification понадобится установить/подключить `pnpm`.

## Fail-fast проверка

Минимальная acceptance-проверка:

1. Запустить приложение локально.
2. Включить запись.
3. Произнести фразу: “Клиент сомневается по цене и срокам”.
4. Увидеть transcript chunk в live transcript.
5. За 5-10 секунд увидеть recommendation card с вопросом или следующим действием.
6. Проверить, что остановка записи не оставляет активных assistant tasks.

## Внешние источники

- Tauri v2 docs: commands и events являются штатным IPC-контуром для связи Rust и frontend.
- OpenRouter docs: `/api/v1/chat/completions` совместим с OpenAI-style chat completions и поддерживает streaming/non-streaming.
- OpenAI docs: Chat Completions поддерживают streaming через event stream; для нового agents-style контура OpenAI рекомендует Responses API, но текущий Meetily client уже построен вокруг Chat Completions-compatible схемы.
