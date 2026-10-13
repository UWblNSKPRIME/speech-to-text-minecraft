# PLAN — голос → чат Minecraft (vocatalk)

## Цель
Программа на Rust: микрофон + системный звук (Discord) → распознавание речи → Fabric-мод шлёт текст в чат Minecraft 26.2. Минимальная задержка, живой стриминг.

## Зафиксированные решения
- Движок: **gigastt-core** (GigaAM v3, ONNX Runtime, CPU), голова **e2e_rnnt** (ru + пунктуация). INT8 ~225 МБ качается при первом запуске.
- Встраиваем крейт в приложение (in-process), без отдельного сервера.
- 2 аудио-канала: микрофон (PTT F8) + WASAPI loopback (Discord, всегда включён).
- Silero VAD на каждый канал. Конец предложения = VAD-тишина + пунктуация.
- Протокол TCP 127.0.0.1:47832, JSON-lines: `{"t":"draft|final","speaker":"me"|"discord","text":...}`.
- Fabric-мод (Loom 1.17, Java 25, Fabric Loader 0.19.5): HUD-оверлей черновика + авто-отправка в чат; guard: окно чата/инвентарь открыто → буфер обмена.
- Деплой jar: `<minecraft-instance>/minecraft/mods` (инстанция уже Fabric).
- Только CPU (без Vulkan/CUDA). README: раздел «В будущем: CUDA».
- Английский не нужен (только ru). Диаризация: сейчас общий префикс `Discord:`, поле speaker заложено на будущее.

## Окружение
- Windows, cwd `<repo-root>` (пустая), cargo 1.95, Python 3.13, cmake 4.3.2, VS 18.5 C++, Java 25 (bundled Java 25 runtime), Gradle не установлен (wrapper скачает).
- GPU GTX 1650, CUDA Toolkit/Vulkan SDK НЕ ставить.

## Этапы
- [x] 1. Движок: gigastt-core в проект, CLI `vocatalk --file x.wav`, проверка пунктуации и RTF
- [x] 2. Два канала: cpal (микроф) + wasapi loopback + Silero VAD ×2 + поток partial/final в консоль, замер TTFP
- [ ] 3. Fabric-мод 26.2: сокет, оверлей, авто-отправка, guard'ы; gradlew build → jar в mods
- [ ] 4. Интеграция: мерж, эндпоинт (пауза VAD + пунктуация), PTT, тюнинг порогов
- [ ] 5. README (**EN**): сборка, схема, хоткеи, «Future: CUDA»
- [ ] Качество репо: LICENSE (MIT), .gitignore, README EN, cargo fmt/clippy без замечаний, метаданные в Cargo.toml (description/license/repository)

## Репозиторий и анонимность
- Public repo: https://github.com/UWblNSKPRIME/speech-to-text-minecraft (ветка `main`).
- Документация на **английском** (корневой README + vocatalk/README); этот PLAN.md — на русском (дев-план).
- **Анонимность**: в файлах и коммитах никаких PII — реальных имён, email, юзернеймов Windows, локальных абсолютных путей (плейсхолдеры `<repo-root>`, `<minecraft-instance>`). Git-identity коммитов — только локальный `UWblNSKPRIME <UWblNSKPRIME@users.noreply.github.com>`, глобальный конфиг не трогать.

## Открытые вопросы
- (закрыто) `1.md` (research-заметки про GigaAM) — пользователь решил оставить локально, в репо не публиковать; файл добавлен в `.gitignore`.
- (закрыто) Тестовый wav для этапа 1 — взят из репо gigastt (`golos_00` → `vocatalk/testdata/sample.wav`).
- (закрыто) Поддержка live-диаризации — этап 2 закрыт без неё (`create_state(false)`, общий префикс `Discord:`); включать capability не потребовалось.
