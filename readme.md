# Auto Reverse

Auto Reverse — будущая системная утилита на Rust для автоматического reverse scrolling. Идея: разные устройства ввода могут иметь разное направление скролла. Например, трекпад остается в natural scrolling, а внешняя мышь работает в reversed mode.

Цель по возможностям: повторить feature set Scroll Reverser для macOS и разложить его на маленькие Rust-модули, чтобы проект можно было изучать постепенно.

Сейчас проект свежий: в `src/main.rs` находится стандартный `Hello, world!`. Документы в репозитории фиксируют план, архитектуру и список рекомендаций перед началом реализации.

## Команды

```bash
cargo build
cargo run
cargo check
cargo test
cargo fmt
cargo clippy
```

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
