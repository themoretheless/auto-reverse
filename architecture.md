# Архитектура Auto Reverse

Auto Reverse - системная Rust-утилита для reverse scrolling в стиле Scroll Reverser. Проект уже не scaffold: в `master` влиты последние локальные изменения из `worktree-rust-impl`, есть macOS event tap, TOML-конфиг, CLI, rule resolver, step size, permission checks, raw-input guard и unit tests.

## Текущее состояние

Реализовано:

- `src/main.rs` - тонкий CLI entrypoint: `run`, `doctor`, `init`, `enable`, `disable`, `toggle`, `config-path`, `show-config`, `simulate`.
- `src/lib.rs` - публичный фасад модулей.
- `src/config.rs` - versioned `AppConfig`, TOML store, atomic-ish save через уникальный temp file.
- `src/device.rs` - `DeviceKind` и conservative classifier: non-continuous scroll = mouse, continuous scroll = trackpad.
- `src/input.rs` - нормализованный `ScrollEvent` с `source_pid`.
- `src/scroll.rs` - чистая трансформация scroll delta, step size, raw-input skip, saturating negation.
- `src/permissions.rs` - Accessibility и Input Monitoring checks.
- `src/event_tap.rs` - macOS `CGEventTap` и recovery при disabled tap.
- `recommendation.md` - 500 актуальных пунктов backlog/review.
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

Domain modules не должны импортировать CoreGraphics, UI framework или конкретный storage. Сейчас `scroll.rs` еще содержит CoreGraphics helpers рядом с чистой логикой; это допустимо для первого среза, но следующая итерация должна разделить `scroll::transformer` и `platform::macos::event_fields`.

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
  lib.rs
  main.rs
  config.rs
  device.rs
  input.rs
  scroll.rs
  permissions.rs
  event_tap.rs
  error.rs
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
      event_fields.rs
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
- raw-input skip;
- permission checks;
- unit tests;
- saturating negation;
- unique temp config saves.

Remaining:

- split CoreGraphics helpers out of `scroll.rs`;
- add tests for corrupt config backup;
- add CLI `--source-pid` simulation;
- add event tap install smoke guard;
- document exact config path behavior.

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

Known risks still open:

- no remote configured, so push cannot complete until `origin` is added;
- no menu bar UI yet;
- no real Magic Mouse distinction yet;
- no config migration yet;
- no packaging/signing yet.
