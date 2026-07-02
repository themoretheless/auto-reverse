# Архитектура Auto Reverse

Auto Reverse - системная Rust-утилита для reverse scrolling в стиле Scroll Reverser. Проект уже не scaffold: в `master` влиты последние локальные изменения из `worktree-rust-impl`, есть macOS event tap, TOML-конфиг, CLI, rule resolver, step size, permission checks, raw-input guard, LaunchAgent start at login и unit tests.

## Текущее состояние

Реализовано:

- `src/main.rs` - тонкий CLI entrypoint: `run`, `doctor`, `init`, `enable`, `disable`, `toggle`, `config-path`, `show-config`, `simulate`.
- `src/lib.rs` - публичный фасад с документацией слоев.
- `src/config/` - разделен по ответственности: `schema.rs` (какие настройки ЕСТЬ: поля, defaults, validation, per-device policy) и `store.rs` (где они ЖИВУТ: пути, TOML I/O, atomic save через уникальный temp file). `mod.rs` реэкспортирует `AppConfig`/`ConfigStore`, так что вызывающий код не зависит от внутреннего разбиения.
- `src/device.rs` - `DeviceKind` и conservative classifier: non-continuous scroll = mouse, continuous scroll = trackpad.
- `src/input.rs` - нормализованный `ScrollEvent` с `source_pid`.
- `src/scroll.rs` - ЧИСТАЯ политика реверса без единого импорта CoreGraphics: config + событие на входе, решение на выходе. Компилируется и тестируется без macOS-фреймворков.
- `src/platform/macos/` - вся OS-специфика и unsafe-код в одном месте: `scroll_events.rs` (маппинг полей CGEvent: прочитать событие, записать решение), `permissions.rs` (Accessibility + Input Monitoring TCC), `startup.rs` (LaunchAgent автозапуск), `event_tap.rs` (runtime-цикл CGEventTap, recovery при disabled tap).
- `Cargo.toml` - `core-foundation`/`core-graphics` теперь target-specific dependencies: чистое ядро собирается без них.
- `recommendation.md` - 500 актуальных пунктов backlog/review + верифицированные находки 3 итераций.
- `scroll-reverser-parity.md` - parity-чеклист Scroll Reverser.

Главный текущий gap: Magic Mouse и trackpad пока не различаются в live event tap. Оба дают continuous scroll, поэтому live classifier безопасно считает continuous scroll trackpad-like. Поле `reverse_magic_mouse` есть в config, но пока не имеет практического эффекта без gesture/HID слоя.

## Цель продукта

Auto Reverse должен повторить пользовательские возможности Scroll Reverser, но остаться маленьким, проверяемым и понятным Rust-проектом:

- reverse mouse wheel при сохранении natural trackpad;
- независимые настройки vertical/horizontal axes;
- независимые настройки mouse/trackpad/Magic Mouse/unknown;
- step size для wheel mouse;
- permissions onboarding;
- menu bar utility с settings;
- debug console;
- start at login;
- hide/show menu icon;
- raw-input mode для remote desktop;
- локальная диагностика без сетевой телеметрии;
- аккуратный native macOS UX.

## SOLID-разделение

### Single Responsibility

Каждый модуль должен иметь одну причину для изменения:

- `config` меняется из-за схемы настроек, storage, migration.
- `device` меняется из-за классификации устройств.
- `input` меняется из-за формы нормализованного события.
- `scroll` меняется из-за правил reverse/step-size.
- `permissions` меняется из-за системных privacy checks.
- `event_tap` меняется из-за macOS hook/runtime.
- `ui` появится отдельно и не должен менять domain-логику.
- `telemetry` появится отдельно и не должен жить в hot path.

### Open/Closed

Новые платформы, UI и classifiers должны добавляться через traits и adapter-модули, а не через переписывание `scroll::transform_event`.

Целевые traits:

```rust
pub trait InputListener {
    fn run(&mut self, sink: &mut dyn InputSink) -> AppResult<()>;
}

pub trait PermissionChecker {
    fn status(&self) -> PermissionStatus;
}

pub trait StartupInstaller {
    fn set_start_at_login(&self, enabled: bool) -> AppResult<()>;
}
```

### Liskov Substitution

`MockInputListener`, `MacOsEventTapListener` и будущие adapters должны иметь один контракт: принимать normalized events и отдавать decisions без скрытых side effects в domain layer.

### Interface Segregation

Не нужен большой trait `Platform`. Нужны маленькие интерфейсы:

- `DeviceClassifier`;
- `InputListener`;
- `ScrollEmitter`;
- `PermissionChecker`;
- `StartupInstaller`;
- `MenuBarController`;
- `DiagnosticsSink`.

### Dependency Inversion

Желаемое направление зависимостей:

```text
CLI / UI / macOS adapter
  -> app runtime
    -> config / input / device / scroll / error
```

Domain modules не должны импортировать CoreGraphics, UI framework или конкретный storage. Это разделение уже сделано: `scroll.rs` - чистая политика без CoreGraphics, а все CGEvent-helpers живут в `platform/macos/scroll_events.rs`. Компилятор охраняет границу: macOS-фреймворки объявлены как target-specific dependencies, и импорт CoreGraphics из domain-модуля сразу виден в diff как нарушение слоя.

## DRY-источники правды

Один источник правды нужен для:

- `DeviceKind::as_str`;
- `AppConfig` defaults;
- config schema version;
- permission labels;
- CLI command names;
- parity checklist;
- design tokens;
- error codes;
- diagnostics field names;
- release checklist.

Если строка или enum-кейс повторяется в CLI, UI и docs, его надо вынести в явный helper или reference table.

## Текущая структура

```text
src/
  lib.rs                           фасад и документация слоев
  main.rs                          CLI
  error.rs                         AppError / AppResult
  device.rs                        DeviceKind + conservative classifier
  input.rs                         ScrollEvent
  scroll.rs                        чистая политика реверса (без CoreGraphics)
  config/
    mod.rs                         реэкспорт AppConfig / ConfigStore
    schema.rs                      поля, defaults, validation, policy
    store.rs                       пути, TOML I/O, atomic save
  platform/
    mod.rs                         cfg-gated адаптеры
    macos/
      mod.rs
      scroll_events.rs             маппинг полей CGEvent
      permissions.rs               Accessibility + Input Monitoring TCC
      startup.rs                   LaunchAgent start at login
      event_tap.rs                 CGEventTap runtime
```

## Целевая структура

```text
src/
  app/
    runtime.rs
    command.rs
    state.rs
  config/
    schema.rs
    store.rs
    migration.rs
  device/
    kind.rs
    classifier.rs
    registry.rs
  input/
    event.rs
    source.rs
  scroll/
    transformer.rs
    wheel.rs
    decision.rs
  platform/
    macos/
      event_tap.rs
      permissions.rs
      scroll_events.rs
      startup.rs
    mock.rs
  ui/
    menu_bar.rs
    settings.rs
    diagnostics.rs
    theme.rs
  telemetry/
    ring_buffer.rs
    diagnostics.rs
  error.rs
```

## Runtime-поток

```text
start
  -> load_or_create config
  -> validate config
  -> check Accessibility/Input Monitoring
  -> install event tap
  -> normalize CGEvent into ScrollEvent
  -> classify source
  -> transform event by AppConfig
  -> write changed delta fields
  -> keep/pass-through if disabled, synthetic, injected, or unsupported

enable-startup
  -> resolve current executable
  -> write ~/Library/LaunchAgents/com.auto-reverse.agent.plist
  -> set config.start_at_login = true
  -> report whether LaunchAgent points at this binary
```

## Дизайн продукта

Auto Reverse должен выглядеть как тихая системная утилита, а не как landing page.

Первый экран settings:

- верхняя строка: `Active`, `Paused`, `Needs Permission`, `Degraded`;
- compact device list;
- per-device direction controls;
- vertical/horizontal controls;
- wheel step size slider;
- permissions panel;
- diagnostics panel;
- restore defaults;
- no nested cards;
- native spacing, typography and icon language.

Menu bar:

- left click opens menu/settings;
- right click toggles active state;
- option click opens debug console;
- icon reflects active/paused/blocked;
- hide icon has a recovery route via CLI.

Visual system:

- neutral native background;
- one restrained accent color;
- no decorative gradients;
- no oversized hero;
- text fits compact controls;
- icon buttons for common commands;
- labels for permission and error states;
- dark/light theme follows system.

## Три итерации

### Итерация 1: Core Safety

Status: mostly done.

Done:

- CLI commands;
- config schema;
- event transform;
- step size;
- LaunchAgent start at login;
- raw-input skip;
- permission checks;
- unit tests;
- saturating negation;
- unique temp config saves.

Remaining:

- add tests for corrupt config backup;
- add event tap install smoke guard;
- document exact config path behavior.

Done since this list was written: CoreGraphics helpers moved out of
`scroll.rs` into `platform/macos/scroll_events.rs` (with target-specific
dependencies guarding the boundary); `--source-pid` and `--synthetic`
simulate flags added.

### Итерация 2: Product UX

Goal: usable daily app, not only CLI.

Scope:

- menu bar app;
- preferences window;
- permission onboarding;
- debug console backed by ring buffer;
- start at login;
- show/hide menu bar icon;
- restore defaults;
- manual diagnostics export;
- native settings layout.

Acceptance:

- user can configure mouse reversed and trackpad natural without reading docs;
- missing permissions show clear next action;
- app can be paused and resumed quickly;
- no event hot path logging slowdown.

### Итерация 3: Reliability and Release

Goal: trusted release candidate.

Scope:

- gesture/HID classifier for Magic Mouse vs trackpad;
- wake from sleep recovery;
- config migration;
- packaging and signing;
- update strategy;
- localization;
- release checklist;
- privacy/security review;
- manual QA matrix.

Acceptance:

- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, `cargo check` pass;
- first-launch, permission-denied, mouse-only, trackpad-only and remote-desktop scenarios are manually tested;
- known limitations are visible in README and settings.

## Review Notes

Issues fixed after the latest merge:

- merge conflict in `src/config.rs` resolved without losing unique temp save IDs;
- stale docs updated to match current CLI/core reality;
- old "Hello, world" recommendations replaced with current audit.
- repeated review fixed the recommendation counter so exactly 500 backlog items are counted;
- `.idea/` is ignored at repository root, keeping IDE metadata out of commits.

Known risks still open:

- no remote configured, so push cannot complete until `origin` is added;
- no menu bar UI yet;
- no real Magic Mouse distinction yet;
- no config migration yet;
- no packaging/signing yet.
