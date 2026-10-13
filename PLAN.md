# PLAN — голос → чат Minecraft (vocatalk)

## Цель
Программа на Rust: микрофон + системный звук (Discord) → распознавание речи → Fabric-мод шлёт текст в чат Minecraft 26.2. Минимальная задержка, живой стриминг.

## Зафиксированные решения
- Движок: **gigastt-core** (GigaAM v3, ONNX Runtime, CPU), голова **e2e_rnnt** (ru + пунктуация). INT8 ~225 МБ качается при первом запуске.
- Встраиваем крейт в приложение (in-process), без отдельного сервера.
- 2 аудио-канала: микрофон (PTT F8) + WASAPI loopback (Discord, всегда включён).
- Silero VAD на каждый канал. Конец предложения = VAD-тишина + пунктуация.
- Протокол TCP 127.0.0.1:47832, JSON-lines: `{"t":"draft|final","speaker":"me"|"discord","text":...}`.
- Fabric-мод (Loom 1.18.4, плагин `net.fabricmc.fabric-loom`, Gradle wrapper 9.7.1, Java 25, Fabric Loader 0.19.5, Fabric API 0.161.0+26.2; MC 26.x необфусцирован — маппинги не нужны): HUD-оверлей черновика + авто-отправка в чат; guard: окно чата/инвентарь открыто или нет мира → буфер обмена + короткое HUD-уведомление.
- Деплой jar: `<minecraft-instance>/minecraft/mods` (инстанция уже Fabric).
- Только CPU (без Vulkan/CUDA). README: раздел «В будущем: CUDA».
- Английский не нужен (только ru). Диаризация: подписи каналов задаются CLI-флагами (`--name-me`/`--name-discord`), поле speaker заложено на будущее.
- **Подписи говорящих** (правка после ревью-этапов): имена задаются CLI-флагами `--name-me` / `--name-discord` (дефолты «Вэвси» / «Аури»), префикс ставит Rust перед отправкой в мод; мод текст шлёт как есть (slash-guard сохранён: сырая строка с «/» уходит без префикса → clipboard). Оба канала распознаются одновременно (пул 2 сессий). — реализовано

## Окружение
- Windows, cwd `<repo-root>` (пустая), cargo 1.95, Python 3.13, cmake 4.3.2, VS 18.5 C++, Java 25 (bundled Java 25 runtime), Gradle не установлен (wrapper скачает).
- GPU GTX 1650, CUDA Toolkit/Vulkan SDK НЕ ставить.

## Этапы
- [x] 1. Движок: gigastt-core в проект, CLI `vocatalk --file x.wav`, проверка пунктуации и RTF
- [x] 2. Два канала: cpal (микроф) + wasapi loopback + Silero VAD ×2 + поток partial/final в консоль, замер TTFP (коммит 4301b8c; ожидает live-смоук с речью от пользователя)
- [x] 3. Fabric-мод 26.2: TCP-слушатель 127.0.0.1:47832 (JSON-lines), HUD-оверлей черновиков (me — белый, discord — зелёный, таймаут 10 с), авто-отправка final с guard'ами (чат/инвентарь/нет мира → буфер обмена + HUD-уведомление), обрезка до 250 символов; gradlew build → jar скопирован в mods — ожидает смоук в игре от пользователя
- [x] 4. Интеграция: TCP-мост в Rust → мод (127.0.0.1:47832, JSON-lines, reconnect раз в 2 с, очередь 100 строк со сбросом устаревшего при реконнекте, try_send — воркеры не блокируются), PTT F8 (global-hotkey; гейтинг аудио канала me, force-finalize хвоста через flush_state на release, статус [ptt] held/released на смену), CLI-флаги --vad-threshold (0.5) / --vad-min-silence-ms (600) — 22 теста, смоуки TCP со слушателем и без; энд-ту-энд в игре — за пользователем
- [x] 5. Документация (**EN**): корневой README — витрина (архитектура, quick start, hotkeys, limitations, troubleshooting, CUDA), vocatalk/README — детали CLI (флаги, бенчмарки, пайплайн), mod/README — протокол и гарды; ручные смоуки этапов 2/3/4 (речь с F8 в игре, оверлей/гарды мода, end-to-end доставка в чат) — за пользователем
- [x] Качество репо: LICENSE (MIT), .gitignore, README EN, cargo fmt/clippy без замечаний, метаданные в Cargo.toml (description/license/repository)
- [x] Ревью-фикс (should-fix + ниты): убрана двойная печать статистики, slash-guard в моде (`/` → буфер обмена), try_send в cpal-колбэках, TranscriptServer (лимит 8 КБ, catch Throwable, soTimeout 300 с), обрезка по границе кодовой точки, тень у notice, README про сокет в onInitializeClient — ревью-фикс закоммичен

## Репозиторий и анонимность
- Public repo: https://github.com/UWblNSKPRIME/speech-to-text-minecraft (ветка `main`).
- Документация на **английском** (корневой README + vocatalk/README); этот PLAN.md — на русском (дев-план).
- **Анонимность**: в файлах и коммитах никаких PII — реальных имён, email, юзернеймов Windows, локальных абсолютных путей (плейсхолдеры `<repo-root>`, `<minecraft-instance>`). Git-identity коммитов — только локальный `UWblNSKPRIME <UWblNSKPRIME@users.noreply.github.com>`, глобальный конфиг не трогать.

## Открытые вопросы
- (закрыто) `1.md` (research-заметки про GigaAM) — пользователь решил оставить локально, в репо не публиковать; файл добавлен в `.gitignore`.
- (закрыто) Тестовый wav для этапа 1 — взят из репо gigastt (`golos_00` → `vocatalk/testdata/sample.wav`).
- (закрыто) Поддержка live-диаризации — этап 2 закрыт без неё (`create_state(false)`, общий префикс `Discord:`); включать capability не потребовалось.
- (закрыто) Маппинги для MC 26.2 — с 26.1 Mojang выпускает необфусцированный jar (маппингов нет вообще); yarn заморожен на 1.21.11. Loom-плагин для таких версий — `net.fabricmc.fabric-loom` (старый id `fabric-loom` — legacy-режим, требует mappings). Fabric API 26.2: `HudRenderCallback` удалён, вместо него `HudElementRegistry` + `HudElement.extractRenderState(GuiGraphicsExtractor, DeltaTracker)`.
