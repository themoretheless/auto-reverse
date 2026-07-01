# Auto Reverse

Auto Reverse — будущая системная утилита на Rust для автоматического reverse scrolling. Идея: разные устройства ввода могут иметь разное направление скролла. Например, трекпад остается в natural scrolling, а внешняя мышь работает в reversed mode.

Цель по возможностям: повторить feature set Scroll Reverser для macOS и разложить его на маленькие Rust-модули, чтобы проект можно было изучать постепенно.

Сейчас реализован первый рабочий срез: macOS event tap, конфиг, rule resolver, scroll transformer, step size для wheel mouse, CLI-команды и unit tests. GUI/menu bar пока в roadmap.

## Команды

```bash
cargo build
cargo run
cargo check
cargo test
cargo fmt
cargo clippy
```

## CLI

```bash
cargo run -- doctor
cargo run -- show-config
cargo run -- simulate --device mouse --dy 1 --dx 2 --continuous false
cargo run -- enable
cargo run -- disable
cargo run -- toggle
cargo run -- run
```

`run` запускает macOS scroll event tap. Для него нужны permissions:

- System Settings -> Privacy & Security -> Accessibility;
- System Settings -> Privacy & Security -> Input Monitoring.

Для безопасных проверок без системного hook используй `doctor`, `show-config` и `simulate`.

## Что уже реализовано

- Конфиг `config.toml` с versioned schema.
- Глобальный `enabled`.
- `reverse_vertical` и `reverse_horizontal`.
- `reverse_mouse`, `reverse_trackpad`, `reverse_magic_mouse`, `reverse_unknown`.
- `discrete_scroll_step_size` для wheel mouse.
- CLI `doctor`, `init`, `enable`, `disable`, `toggle`, `show-config`, `simulate`.
- CoreGraphics event tap для macOS.
- Safe pass-through при disabled config.
- Conservative classifier: physical wheel = mouse, continuous scroll = trackpad-like.
- Source classifier по модели Scroll Reverser подготовлен для будущих gesture events.
- Unit tests для конфига, classifier и scroll transform.

Текущий важный gap: Magic Mouse и trackpad пока не разделяются на уровне реального event tap, потому что для этого нужен следующий слой gesture tracking.

## Пользовательский сценарий

Пользователь открывает приложение, видит список устройств и выбирает направление скролла для каждого:

- `Trackpad` -> `Natural`;
- `Mouse` -> `Reversed`;
- `Unknown device` -> default rule;
- `Pause` -> временно отключить обработку;
- `Diagnostics` -> проверить права доступа и последние события.

## Feature parity

Auto Reverse должен повторить пользовательские возможности Scroll Reverser:

- reverse scrolling для mouse/trackpad/Magic Mouse;
- независимые настройки vertical и horizontal axes;
- глобальный enable/disable;
- step size slider для wheel mouse;
- menu bar/tray icon;
- preferences window;
- permissions onboarding для Accessibility/Input Monitoring;
- start at login;
- show/hide menu bar icon;
- debug console;
- update checks;
- dark mode/native visual style;
- localization;
- documented limitations for gestures and custom scroll surfaces.

Полный parity checklist: `scroll-reverser-parity.md`.

## План разработки

### Итерация 1: ядро

- Описать domain types.
- Добавить config schema.
- Добавить rule resolver.
- Добавить transformer scroll delta.
- Добавить mock platform.
- Добавить Scroll Reverser parity checklist в acceptance criteria.
- Покрыть правила unit tests.

### Итерация 2: продукт

- Добавить tray/menu bar.
- Добавить screen настроек.
- Добавить onboarding по permissions.
- Добавить step size для wheel mouse.
- Добавить start at login и hide/show menu icon.
- Добавить debug console.
- Добавить device registry.
- Добавить профили.
- Добавить локальную диагностику.

### Итерация 3: релиз

- Добавить integration tests.
- Добавить packaging.
- Добавить migration настроек.
- Добавить crash recovery.
- Подготовить release checklist.
- Добавить update strategy и localization workflow.
- Провести security/privacy review.

## Архитектурные принципы

- Core-логика не зависит от GUI и ОС.
- Платформенные API спрятаны за traits.
- Конфиг имеет versioned schema.
- Каждый модуль имеет одну ответственность.
- Повторяющиеся строки, enum и правила выносятся в один источник правды.
- Сначала пишется тестируемая логика, потом системная интеграция.

## Предлагаемые модули

```text
src/app        orchestration and lifecycle
src/config     settings schema, storage, migration
src/device     device id, classification, registry
src/input      normalized input events
src/scroll     direction, rules, transformer
src/platform   macOS, Windows, Linux adapters
src/ui         tray/settings UI
src/telemetry  local logs and diagnostics
src/error.rs   shared application errors
```

## Дизайн интерфейса

Приложение должно ощущаться как аккуратная системная утилита, а не как рекламная страница. Первый экран — список устройств и их состояние. Основные controls:

- переключатель активности;
- направление скролла на устройство;
- статус permissions;
- кнопка диагностики;
- ссылка на logs;
- restore defaults.

Визуально: компактно, спокойно, с хорошими отступами, без декоративных градиентов и лишних карточек. Для кнопок использовать привычные icons, а текст оставлять там, где он действительно объясняет состояние.

## Документы

- `architecture.md` — целевая архитектура и разделение по SOLID/DRY.
- `scroll-reverser-parity.md` — полный список фич Scroll Reverser, которые нужно повторить.
- `recommendation.md` — 500 пунктов: предложения, проблемы, улучшения, ошибки и риски.
- `readme.md` — краткая точка входа в проект.
