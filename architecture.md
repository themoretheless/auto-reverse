# Архитектура Auto Reverse

Auto Reverse - системная Rust-утилита для reverse scrolling в стиле Scroll Reverser. Проект уже не scaffold: в `master` влиты последние локальные изменения из `worktree-rust-impl`, есть macOS event tap, TOML-конфиг, CLI, отдельный parser команд, rule resolver, step size, permission checks, raw-input guard, LaunchAgent start at login и unit tests.

## Текущее состояние

Реализовано:

- `src/main.rs` - тонкий CLI entrypoint/orchestrator: запускает команды, но не парсит флаги вручную.
- `src/cli.rs` - маленький parser команд и флагов: `run`, `ui`, `doctor --no-create`, `init`, `enable`, `disable`, `toggle`, `enable-startup`, `disable-startup`, `startup-status --json`, `devices`, `config-path`, `show-config`, `simulate` (включая `--vendor-id`/`--product-id`).
- `src/ui.rs` - egui settings window (feature `gui`, включён по умолчанию), слитый с event tap в один процесс: открытие окна запускает `CGEventTap` на фоновом потоке (`event_tap::install_and_run`), деля один `Arc<RwLock<AppConfig>>` между окном и потоком - изменение любого чекбокса применяется к следующему scroll-событию немедленно, без рестарта. Если приложение открыли до выдачи Accessibility/Input Monitoring, UI оставляет старт tap pending и автоматически повторяет запуск, когда оба permission-check становятся зелеными. `install_and_run` сам берет `daemon_lock` первым делом, так что headless `run` и этот in-process поток никогда не держат живой tap одновременно. Иконка в menu bar (`platform::macos::tray`) живет всё время процесса; это native AppKit template status icon, поэтому AppKit сам подбирает контраст для светлой/темной строки меню. Закрытие окна (красная кнопка/Cmd-W) перехватывается через `ViewportCommand::CancelClose` и прячет окно, а не завершает процесс - только Quit из трея реально вызывает `std::process::exit`. "Start at login" в окне использует `platform::macos::login_item` (`SMAppService.mainAppService()`), отдельно от CLI-механизма `startup.rs`/`enable-startup`.
- `src/lib.rs` - публичный фасад с документацией слоев.
- `src/config/` - разделен по ответственности: `schema.rs` (какие настройки ЕСТЬ: поля, defaults, validation, per-device policy) и `store.rs` (где они ЖИВУТ: пути, TOML I/O, atomic save через уникальный temp file). `mod.rs` реэкспортирует `AppConfig`/`ConfigStore`, так что вызывающий код не зависит от внутреннего разбиения.
- `src/device.rs` - `DeviceKind` и conservative classifier: non-continuous scroll = mouse, continuous scroll = trackpad.
- `src/input.rs` - нормализованный `ScrollEvent` с `source_pid`.
- `src/scroll.rs` - ЧИСТАЯ политика реверса без единого импорта CoreGraphics: config + событие на входе, решение на выходе. Компилируется и тестируется без macOS-фреймворков.
- `src/platform/macos/` - вся OS-специфика и unsafe-код в одном месте: `scroll_events.rs` (маппинг полей CGEvent: прочитать событие, записать решение), `permissions.rs` (Accessibility + Input Monitoring TCC), `hid.rs` (IOHIDManager wheel monitor: атрибуция дискретного скролла конкретному vendor/product ID для `device_rules`), `startup.rs` (LaunchAgent автозапуск для headless CLI: `enable-startup`/`disable-startup`, таргетит `run`), `event_tap.rs` (runtime-цикл CGEventTap; конфиг теперь `Arc<RwLock<AppConfig>>` вместо `OnceLock<AppConfig>`, так что GUI-поток может писать изменения, которые следующее scroll-событие увидит сразу; `install_and_run` сам берет `daemon_lock` перед HID-монитором и созданием tap), `daemon_lock.rs` (`flock`-lock: гарантирует, что никогда не запущено два живых `CGEventTap` одновременно, независимо от способа запуска - ручной `run`, LaunchAgent, headless или in-process поток внутри `ui` - все берут один и тот же лок-файл), `login_item.rs` (только `gui`: тонкая обертка над `SMAppService.mainAppService()` для регистрации `.app`-бандла как login item - отдельный от `startup.rs` механизм для отдельного сценария, см. риск №6 в `recommendation.md`), `tray.rs` (только `gui`: native AppKit template icon в menu bar, меню Open Settings/Quit).
- `Cargo.toml` - `core-foundation`/`core-graphics` теперь target-specific dependencies: чистое ядро собирается без них. `eframe`/`objc2-app-kit`/`objc2-service-management`/`objc2-foundation`/`objc2` - тоже target-specific (macOS) И под фичей `gui`, так что `cargo build --no-default-features` их не тянет вообще.
- `scripts/build-app-bundle.sh` - локальный `.app` bundle для macOS Privacy & Security; копирует реальный Mach-O бинарь в `Contents/MacOS/auto-reverse` без shell-wrapper, двойной клик открывает settings window (`ui`), которое теперь запускает event tap в этом же процессе и держит иконку в menu bar (см. выше).
- `recommendation.md` - 660 пунктов backlog/review (500 базовых + N01-N160 после автозапуска, menu bar и bundle identity) + верифицированные находки итераций ревью.
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
- `cli` меняется из-за команд, флагов и форматов вывода.
- `device` меняется из-за классификации устройств.
- `input` меняется из-за формы нормализованного события.
- `scroll` меняется из-за правил reverse/step-size.
- `permissions` меняется из-за системных privacy checks.
- `event_tap` меняется из-за macOS hook/runtime.
- `ui` уже есть, но его следующие крупные изменения должны дробиться на `ui/settings`, `ui/menu_bar`, `ui/diagnostics`, а не тащить domain-логику внутрь egui-кода.
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
- CLI command names and accepted flag values in `src/cli.rs`;
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
  main.rs                          CLI orchestration
  cli.rs                           command/flag parser, options, parser tests
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
      hid.rs                       IOHIDManager: атрибуция скролла конкретному устройству
      permissions.rs               Accessibility + Input Monitoring TCC
      startup.rs                   LaunchAgent start at login (headless `run`)
      event_tap.rs                 CGEventTap runtime, config shared via Arc<RwLock<_>>
      daemon_lock.rs               flock: only one live CGEventTap at a time, any launch path
      login_item.rs                SMAppService.mainAppService() wrapper (gui feature only)
      tray.rs                      menu-bar tray icon (gui feature only)
scripts/
  build-app-bundle.sh              создает target/debug или target/release Auto Reverse.app
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
  -> check/request Accessibility/Input Monitoring
  -> if permissions are ready: install event tap
  -> if permissions are missing: keep UI open and retry after they become ready
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

scripts/build-app-bundle.sh
  -> cargo build
  -> create target/<profile>/Auto Reverse.app
  -> copy auto-reverse into Contents/MacOS
  -> write Info.plist with LSUIElement=true
  -> ad-hoc codesign when codesign is available

doctor --no-create
  -> use defaults if config is missing
  -> do not write a config file as a side effect
  -> print binary path, config state, permissions, startup status and known gaps

startup-status --json
  -> read LaunchAgent and config state
  -> do not create config
  -> print machine-readable installed/config/in_sync fields
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
- CLI parser split into `src/cli.rs`;
- `doctor --no-create`;
- `startup-status --json`;
- local GUI `.app` bundle for Security settings and daily use;
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
- automatic tap retry after permissions become ready;
- debug console backed by ring buffer;
- start at login UI toggle (CLI enable-startup/disable-startup уже сделаны в Итерации 1);
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
- repeated review fixed the recommendation counter so exactly 500 backlog items are counted (later extended by N01-N160 after startup, menu bar and bundle identity work, total 660);
- `.idea/` is ignored at repository root, keeping IDE metadata out of commits.
- SOLID/DRY follow-up split CLI parsing from `main.rs`, added no-side-effect diagnostics, and added machine-readable startup status.
- menu bar icon now uses a macOS template alpha mask instead of a dark solid
  square, so AppKit can tint it correctly on light/dark menu bars.
- bundle launch identity was corrected: `CFBundleExecutable` is the real Mach-O
  binary (`Contents/MacOS/auto-reverse`), not a shell wrapper that execs a
  differently named process.
- 2026-07-04 review fixed a first-run UX bug: after granting permissions while
  the settings window is already open, the UI now retries starting the tap
  automatically instead of requiring a manual toggle.

Known risks still open:

- menu bar UI is now a minimal working tray icon (Open Settings/Quit only) -
  the richer roadmap vision (state-reflecting icon, right-click toggle,
  debug console) is still open;
- no real Magic Mouse distinction yet;
- no config migration yet;
- no production packaging/notarized signing yet.
