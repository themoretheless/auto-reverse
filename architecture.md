# Архитектура Auto Reverse

Auto Reverse - системная Rust-утилита для reverse scrolling в стиле Scroll Reverser. Проект уже не scaffold: в `master` влиты последние локальные изменения из `worktree-rust-impl`, есть macOS event tap, TOML-конфиг, CLI, отдельный parser команд, rule resolver, step size, permission checks, raw-input guard, LaunchAgent start at login и unit tests.

## Текущее состояние

Реализовано:

- `src/main.rs` - тонкий CLI entrypoint/orchestrator: запускает команды, но не парсит флаги вручную.
- `src/cli.rs` - маленький parser команд и флагов: `run`, `ui`, `benchmark`, `doctor --no-create`, `init`, `enable`, `disable`, `toggle`, `enable-startup`, `disable-startup`, `startup-status --json`, internal-safe `prepare-uninstall`, `devices`, `config-path`, `show-config`, `simulate` и `trace-lab`.
- `src/ui.rs` - coordinator egui settings app: владеет config/store/tray и собирает вкладки General/Devices/Permissions. Тяжелые ответственности вынесены: `ui/runtime.rs` типизирует tap lifecycle и bounded wake recovery, `ui/theme.rs` хранит handoff tokens/custom controls, `ui/debug_console.rs` владеет diagnostics viewport, `ui/debug_console/export.rs` - trace/CSV workflow, `ui/scroll_benchmark.rs` - отдельный benchmark viewport, а `ui/local_export.rs` - общий atomic local write/CSV escaping. Окно и event tap делят `Arc<RwLock<AppConfig>>`; изменения применяются к следующему событию, а process-local `RuntimeControl` дает pause/resume без записи TOML. UI и tray сохраняют только поверх точной загруженной TOML-ревизии: stale write отклоняется, новый внешний config перечитывается, пользователь повторяет локальное действие. Missing permissions сразу открывают Permissions tab, Restore defaults требует подтверждения. `ui.lock` запрещает дубли окна/иконки; второй launch через `activation.rs` адресует владельцу PID mailbox-запрос и открывает/фокусирует скрытое окно. Cmd-W/Cmd-Q скрывают окно, только tray Quit завершает процесс.
- `src/lib.rs` - публичный фасад с документацией слоев.
- `src/config/` - разделен по ответственности: `schema.rs` хранит поля/defaults/validation, `device_rules.rs` - чистый приоритет serial > location > legacy vendor/product, matching и общую мутацию для UI/tray, `store.rs` - пути, TOML I/O, atomic save через уникальный temp file, persistent cross-process lock, exact revision snapshots/CAS и locked `update`. `mod.rs` реэкспортирует стабильный facade.
- `src/device.rs` - только словарь: `DeviceKind`, `HardwareId` и best-available `DeviceIdentity`.
- `src/device_classifier.rs` - чистая state machine без AppKit: discrete = mouse, exclusive IOHID inventory фиксирует Trackpad/Magic Mouse, а two-finger timing и momentum разрешают только реальный `Both`; unknown безопасно становится Trackpad.
- `src/input.rs` - нормализованный `ScrollEvent` с `source_pid` и optional shared `Arc<DeviceIdentity>`.
- `src/diagnostics.rs` - чистый общий словарь axis/category/decision reason, используемый Debug Console и trace без зависимости от macOS.
- `src/runtime.rs` - чистый lock-free `RuntimeControl` для временной паузы; UI, tray и event tap читают один процессный state.
- `src/scroll.rs` - ЧИСТАЯ политика реверса без единого импорта CoreGraphics: config + событие на входе, решение на выходе. Компилируется и тестируется без macOS-фреймворков.
- `src/scroll_trace.rs` - versioned privacy-bounded TOML schema, validation и deterministic pure replay; PID, device identity и wall time в формат не входят.
- `src/scroll_lab.rs` - pure magnitude/interval/direction/duration/clutch metrics, axis totals, observed event rates и обязательный constant-gain baseline.
- `src/statistics.rs` и `src/event_rate.rs` - общая nearest-rank integer distribution и чистая observed-arrival аналитика по device kind; idle gaps не превращаются в ложный низкий polling rate.
- `src/latency_budget.rs` - чистые callback/scheduler engineering budgets и bounded history: оценка начинается после трех readings, warning требует два нарушения среди последних пяти; interval max и average reading не смешиваются терминологически.
- `src/scroll_benchmark.rs` - чистые matrix bounds и ScrollTest-style state machine: шесть физических test strata, Known/Unknown, 66 ms settle, movement time, switchbacks и maximum overshoot.
- `src/scroll_dynamics.rs` - чистая experimental scalar-axis state machine и presets Off/Precise/Balanced/Fast; она не подключена к live event tap, не владеет timer/thread/config/CoreGraphics.
- `src/platform/macos/` - вся OS-специфика и unsafe-код: `scroll_events.rs` владеет полями CGEvent, `permissions.rs` - TCC, `hid.rs` - IOHIDManager inventory, serial/location reads и единый `WheelSnapshot`, `gesture.rs` - отдельный listen-only public AppKit tap и перевод touch/momentum observations в чистый classifier, `event_tap.rs` - lock/install/readiness/run loop и lifetime-safe registry обоих tap ports, `tap_metrics.rs` - bounded on-demand `CGGetEventTapList`, остальные adapters сохраняют прежние узкие ответственности.
- `Cargo.toml` - macOS framework dependencies target-specific. Lean `--no-default-features` сохраняет только минимальные AppKit NSEvent/NSTouch bindings для классификатора; eframe, окна, меню, изображения и login item остаются строго под `gui`.
- `scripts/` - `build-app-bundle.sh` строит `.app` с реальным Mach-O, Retina icon, versioned plist, least-privilege entitlements и hardened signature; `check-app-bundle.sh` разделяет identity/local/release/notarized validation; `release-app-bundle.sh` оркестрирует Developer ID, secure timestamp, `notarytool`, stapling, Gatekeeper и checksummed ZIP; installer/uninstaller сохраняют staged same-volume rollback и exact identity; оба workflow имеют отдельные smoke scripts.
- `tests/cli_integration.rs` - black-box запуск собранного binary через `std::process::Command`: каждый test получает отдельный `HOME`, очищает inherited path overrides и проверяет default/explicit config paths, no-create diagnostics и конкурентные CLI/startup writes без доступа к реальному профилю.
- `recommendation.md` - 960 пунктов backlog/review (500 базовых + N01-N400 + R01-R60); `ROADMAP.md` сворачивает их в 25 исполнимых P0/P1/P2 задач.
- `RESEARCH.md` - разбор 10 дополнительных macOS/input repositories, научных работ и platform materials с приоритетом измерения до изменения scroll dynamics.
- `DESIGN.md`, `QA.md`, `PRIVACY.md`, `SECURITY.md`, `RELEASE.md`, `TRACE.md`, `BENCHMARK.md`, `DYNAMICS.md`, `CONTRIBUTING.md` - отдельные контракты вместо смешивания release/design/measurement требований с architecture narrative; `RELEASE.md` является единственным полным release checklist.
- `scroll-reverser-parity.md` - parity-чеклист Scroll Reverser.

Magic Mouse и trackpad различаются inventory-first: публичные IOHID product names сначала определяют `TrackpadOnly`, `MagicMouseOnly`, `Both` или `Unknown`, а matching/removal callbacks обновляют атомарный hint при hot-plug и sleep/wake. Exclusive hint не зависит от ненадёжного touch-count; только `Both` использует пассивный `NSEventTypeGesture` tap и timing/momentum state machine. Это исправляет реальную регрессию, где единственный встроенный trackpad управлялся checkbox Magic Mouse из-за отсутствующего two-finger observation. При unknown/failed tap continuous scroll безопасно считается Trackpad. Быстрое чередование обоих устройств и сторонние smooth wheels остаются heuristic edge cases. Для дискретных колёс identity v2 использует serial или port/location fallback.

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
- `device` меняется из-за словаря и identity устройств.
- `device_classifier` меняется из-за чистой timing/gesture политики.
- `input` меняется из-за формы нормализованного события.
- `scroll` меняется из-за правил reverse/step-size.
- `permissions` меняется из-за системных privacy checks.
- `event_tap` меняется из-за macOS hook/runtime.
- `ui` уже разделен: Debug Console, trace/CSV export, benchmark viewport и shared local export не смешаны, а native dialog/Finder calls живут в `platform/macos/save_panel.rs`; следующие крупные изменения должны сохранять эту границу.
- `scroll_benchmark` не знает egui и macOS: UI передает timestamp и document delta, state machine возвращает immutable `TrialResult`.
- `tap_metrics` не знает Debug Console: адаптер возвращает typed interval snapshot, а UI сам решает, когда пользователь явно запросил сбрасывающее min/max чтение.
- `debug_log` хранит GUI-only local diagnostics как structured events: callback записывает enum reason, source PID, synthetic flag, device kind/hardware/name и deltas, а строки строятся только при search/render/export. Следующий SOLID-шаг - маленький `DiagnosticsSink`, чтобы event tap не знал конкретный ring buffer.

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
- release checklist в `RELEASE.md`.

Если строка или enum-кейс повторяется в CLI, UI и docs, его надо вынести в явный helper или reference table.

## Текущая структура

```text
src/
  lib.rs                           фасад и документация слоев
  main.rs                          CLI orchestration
  cli.rs                           command/flag parser, options, parser tests
  error.rs                         AppError / AppResult
  device.rs                        DeviceKind + HardwareId/DeviceIdentity vocabulary
  device_classifier.rs             pure inventory/gesture/timing classifier + fallback
  input.rs                         ScrollEvent
  runtime.rs                       process-local temporary pause (AtomicU64)
  scroll.rs                        чистая политика реверса (без CoreGraphics)
  statistics.rs                    shared nearest-rank integer distributions
  event_rate.rs                    observed per-device arrival-rate distributions
  scroll_trace.rs                  privacy-bounded TOML schema + replay
  scroll_lab.rs                    transfer metrics, rates and baseline
  scroll_benchmark.rs              pure target-acquisition state machine
  ui.rs                            settings coordinator and tab contents
  ui/
    runtime.rs                     typed tap lifecycle + event channel
    theme.rs                       handoff tokens and custom egui controls
    local_export.rs                shared CSV escaping + atomic local replace
    scroll_benchmark.rs            benchmark viewport/input adapter/result CSV
    debug_console.rs               diagnostics viewport/filter/table
    debug_console/
      export.rs                    detailed CSV/privacy trace workflow + receipt
  config/
    mod.rs                         реэкспорт AppConfig / ConfigStore
    schema.rs                      поля, defaults, validation
    device_rules.rs                pure identity matching, priority and mutation
    store.rs                       TOML I/O, atomic save, file lock, snapshots/CAS
  platform/
    mod.rs                         cfg-gated адаптеры
    macos/
      mod.rs
      scroll_events.rs             маппинг полей CGEvent
      hid.rs                       IOHIDManager: continuous inventory + wheel identity/attribution
      gesture.rs                   passive public AppKit gesture tap + classifier adapter
      permissions.rs               Accessibility TCC policy/check/request
      startup.rs                   LaunchAgent start at login (headless `run`)
      event_tap.rs                 CGEventTap runtime, config shared via Arc<RwLock<_>>
      power_events.rs              NSWorkspace sleep/wake observer (gui only)
      debug_log.rs                 structured events + локальный ring buffer (gui only)
      daemon_lock.rs               flock: only one live CGEventTap at a time, any launch path
      activation.rs                second GUI launch -> focus mailbox (gui only)
      save_panel.rs                native export destination + Finder reveal (gui only)
      tap_metrics.rs               on-demand CGGetEventTapList interval snapshot
      quit_handler.rs              AppleEvent quit interception (Cmd-Q hides, tray Quit exits)
      login_item.rs                SMAppService.mainAppService() wrapper (gui feature only)
      tray.rs                      rich AppKit tray icon/menu (gui feature only)
      tray/
        device_rules.rs            pure three-state quick-pick mutation
scripts/
  lib/app-bundle.sh                shared bundle identity + exact-process helpers
  build-app-bundle.sh              создает hardened local или Developer-ID signed .app
  check-app-bundle.sh              identity/local/release/notarized bundle validation
  release-app-bundle.sh            sign -> notary -> staple -> Gatekeeper -> final ZIP
  check-release-workflow.sh        hardened/negative-gate/side-effect-free plan smoke
  install-app-bundle.sh            staged atomic install/update + rollback
  uninstall-app-bundle.sh          startup cleanup + identity-checked removal
  check-install-workflow.sh        isolated fresh/update/uninstall smoke
packaging/
  AutoReverse.entitlements         намеренно пустой least-privilege plist
tests/
  cli_integration.rs               binary smoke в isolated HOME/config paths
```

## Следующие границы, только когда они окупятся

```text
src/
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
    settings.rs
    devices.rs
    permissions.rs
  diagnostics/
    ring_buffer.rs
    export.rs
    diagnostics.rs
  error.rs
```

## Runtime-поток

```text
start
  -> load_or_create config
  -> validate config
  -> check/request Accessibility (the active tap does not separately require Input Monitoring)
  -> if permissions are ready: install event tap
  -> report Started / AlreadyRunning / Stopped / Failed through ui/runtime channel
  -> on NSWorkspace DidWake: re-check permissions, re-arm a live tap, or restart a stopped tap once within a bounded window
  -> if permissions are missing: keep UI open and retry after they become ready
  -> if user disables reversal: keep a live tap in pass-through mode; if no tap is running, clear the failed/pending start attempt
  -> if user pauses for 15 minutes: keep config unchanged and pass through while RuntimeControl deadline is active
  -> CLI config mutations hold config.toml.lock for the complete read-modify-write transaction
  -> GUI/tray compare the exact loaded TOML revision before save; on conflict reload disk and ask to repeat the edit
  -> if disk save fails: roll UI controls back to the last shared runtime config
  -> if tray toggles Reverse Scrolling: resync the window config and run the same enabled lifecycle helper
  -> IOHID wheel callback builds DeviceIdentity(vendor/product + serial/location), caching Arcs across nearby ticks
  -> serial rule wins over location, location wins over legacy model-wide rule, independent of TOML order
  -> normalize CGEvent into ScrollEvent
  -> snapshot connected Trackpad/Magic Mouse product inventory through IOHID
  -> if exactly one continuous source exists: use that policy directly
  -> otherwise passive gesture tap counts touching fingers without changing the event
  -> classify `Both` from continuity + momentum + recent two-finger observation
  -> if inventory/gesture evidence is unavailable: classify continuous scroll as trackpad
  -> transform event by AppConfig
  -> write changed delta fields
  -> for gui builds: record reason enum + raw source/device fields in the in-memory ring buffer
  -> format device/decision text only when Debug Console searches, renders, or exports
  -> keep/pass-through if disabled, synthetic, injected, or unsupported

second ui launch
  -> acquire_or_activate attempts ui.lock before constructing AppKit objects
  -> if another process owns it: atomically publish its PID to ui.activate and exit successfully
  -> owner consumes the request on the existing 250 ms tick after close handling
  -> send Visible(true) before Focus so winit activates and orders the window front

debug export
  -> snapshot the currently filtered structured events
  -> NSSavePanel returns a user-selected CSV path or a silent Cancel
  -> serialize stable raw fields/reason codes and atomically replace the selected file
  -> keep a structured receipt; NSWorkspace reveals that exact file in Finder on demand

enable-startup
  -> resolve current executable
  -> write ~/Library/LaunchAgents/com.auto-reverse.agent.plist
  -> set config.start_at_login = true
  -> report whether LaunchAgent points at this binary

scripts/build-app-bundle.sh
  -> cargo build
  -> create target/<profile>/Auto Reverse.app
  -> copy auto-reverse into Contents/MacOS
  -> render assets/AppIcon.svg into Retina iconset + AutoReverse.icns
  -> write versioned Info.plist with LSUIElement=true and CFBundleIconFile
  -> sign with hardened runtime and explicit least-privilege entitlements
  -> default to ad-hoc for local work; add secure timestamp for an explicit identity
  -> check Mach-O, plist, icon, identifier and signature via check-app-bundle.sh

scripts/release-app-bundle.sh
  -> require a signing identity and notarytool Keychain profile
  -> build release and reject any authority except Developer ID Application
  -> require hardened runtime + secure timestamp before upload
  -> archive with ditto; submit with notarytool --wait; require Accepted
  -> persist result/log, staple and validate ticket, assess through Gatekeeper
  -> create a staged final ZIP + SHA-256 and replace the prior artifact only after all gates pass

scripts/install-app-bundle.sh
  -> build release by default and strict-check source
  -> identity-check an existing destination and stop only its exact executable PIDs
  -> ditto into a sibling stage; strict-check stage
  -> move old destination to sibling backup and stage into the stable path
  -> strict-check installed result; rollback backup on failure
  -> open the exact installed bundle and verify that its process stays alive

scripts/uninstall-app-bundle.sh
  -> identity-check the destination; refuse symlinks/unexpected names
  -> invoke bundled prepare-uninstall for SMAppService + LaunchAgent cleanup
  -> stop only exact installed executable PIDs and remove the app
  -> retain config/rules by default; remove owned config/locks/log only by explicit flag

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

- pinned status: `ON`, `OFF`, `PAUSED`, `NEEDS PERMISSION`, плюс inline runtime errors;
- compact device list;
- three separate live policy controls: Mouse wheel, Trackpad, Magic Mouse;
- per-device direction controls keyed by serial, with port fallback and explicit inherited legacy rules;
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
- icon reflects active/paused/permission-blocked through a separate status dot;
- rich native menu exposes Reverse Scrolling, Devices, Open Settings, Open Debug Console, Quit;
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

## Следующий research-derived слой

Дополнительный проход по Stats, MonitorControl, Rectangle,
Karabiner-Elements, Mos, AltTab, Hammerspoon, Mac Mouse Fix, LinearMouse и
UnnaturalScrollWheels, а также работам по scrolling transfer functions,
accuracy, filtering и latency зафиксирован в `RESEARCH.md`.

Главное архитектурное решение: не добавлять "smooth" прямо в
`event_tap.rs`. Следующий цикл разделен на три маленьких этапа:

1. Measurement: pure trace/replay, transfer-function benchmark и public tap
   latency snapshot без изменения runtime behavior.
2. Dynamics experiment: отдельная pure per-axis state machine и bounded
   scheduler только для discrete wheel, opt-in и fail-open.
3. Profiles/UX: optional preset в существующем `DeviceRule`, inheritance,
   config import dry-run, settings search и device test row.

Measurement boundaries и первая pure dynamics boundary уже реализованы;
platform scheduler/profile модули создаются только в своих итерациях:

```text
src/diagnostics.rs                        stable pure diagnostic vocabulary [done]
src/statistics.rs                         shared nearest-rank distributions [done]
src/scroll_trace.rs                       trace schema and deterministic replay [done]
src/scroll_lab.rs                         transfer metrics and baseline [done]
src/event_rate.rs                         observed arrival-rate histogram [done]
src/scroll_benchmark.rs                   pure ScrollTest-style trial state [done]
src/ui/scroll_benchmark.rs                interactive benchmark viewport [implemented]
src/latency_budget.rs                     repeated-stall budget assessment [done]
src/scroll_dynamics.rs                    pure scalar-axis dynamics [done, not live]
src/config/profiles.rs                    profile inheritance and precedence
src/platform/macos/tap_metrics.rs         CGGetEventTapList adapter [implemented]
src/platform/macos/scroll_scheduler.rs    tagged bounded output scheduler
```

Инварианты остаются жесткими: continuous Trackpad/Magic Mouse path не
сглаживается повторно; inventory-first classification не заменяется
отсутствием touch observations; scheduler пишет только `DeltaAxis1/2`; private
touch APIs, HID seizure и telemetry не используются. Новый backlog имеет IDs
`R01-R60` в `recommendation.md`.

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

Status: production signing/notarization orchestration is implemented; a real
Developer ID certificate, Apple submission, clean-machine Gatekeeper/TCC
continuity, physical-device scenarios, and update strategy remain release QA.

Scope:

- physical Magic Mouse/trackpad, momentum, rapid-alternation and system-gesture QA for the implemented public classifier;
- manual sleep/wake validation of the implemented recovery path;
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

## Top 500 mirror

`recommendation.md` остаётся canonical source: там 500 базовых предложений,
проблем, ошибок и улучшений, follow-up `N01-N400` и research backlog
`R01-R60`. Блок ниже зеркалит именно базовые 1-500, чтобы архитектуру можно
было читать без прыжка в другой файл.

<details>
<summary>Показать 500 базовых пунктов</summary>

<!-- TOP500_ARCHITECTURE:START -->
| # | Type | Item |
| --- | --- | --- |
| 1 | Done | Проект уже имеет рабочий CLI вместо старого `Hello, world!`. |
| 2 | Done | `src/lib.rs` отделяет library facade от binary entrypoint. |
| 3 | Done | `src/main.rs` стал тонким CLI entrypoint. |
| 4 | Done | `AppConfig` хранит versioned config schema. |
| 5 | Done | TOML выбран как читаемый формат настроек. |
| 6 | Done | `ConfigStore::default_path` учитывает macOS Application Support. |
| 7 | Done | `AUTO_REVERSE_CONFIG` помогает безопасно тестировать конфиг. |
| 8 | Done | `load_or_create` делает first-run проще. |
| 9 | Done | Config save использует уникальный temporary file. |
| 10 | Problem | Config save еще не делает fsync файла и директории. |
| 11 | Improve | Добавить durable save для production release. |
| 12 | Problem | Нет backup corrupted config. |
| 13 | Improve | При parse error сохранять `.broken.<timestamp>.toml`. |
| 14 | Problem | Нет migration framework для `config_version`. |
| 15 | Improve | Добавить `config::migration` до schema v2. |
| 16 | Done | `ConfigStore` и `AppConfig` разделены: `config/schema.rs` и `config/store.rs`. |
| 17 | Done | Разделение `config/schema.rs` / `config/store.rs` выполнено. |
| 18 | Done | Монолитный `config.rs` удален; ответственности разнесены по SRP. |
| 19 | Done | `config/mod.rs` реэкспортирует `AppConfig`/`ConfigStore` как public facade. |
| 20 | Done | `DeviceKind::as_str` уменьшает DRY-дублирование. |
| 21 | Done | `Display` и `FromStr` используют единый device-name контракт. |
| 22 | Done | `DeviceKind` покрывает mouse, trackpad, Magic Mouse, unknown. |
| 23 | Done | Magic Mouse определяется live как отдельный best-effort continuous source. |
| 24 | Done | Добавлен отдельный public AppKit gesture classifier без private API. |
| 25 | Done | Continuous source сначала определяется по exclusive IOHID inventory; timing signal нужен только при `Both`. |
| 26 | Done | `doctor`, README и UI честно показывают отдельную Magic Mouse policy и heuristic caveat. |
| 27 | Done | Non-continuous scroll считается mouse. |
| 28 | Done | `DeviceIdentity` использует serial, затем port/location fallback. |
| 29 | Done | `DeviceIdentity` и `DeviceInfo` проведены через HID, policy, CLI, UI и tray. |
| 30 | Problem | Нет device registry. |
| 31 | Improve | Хранить known devices и last_seen metadata. |
| 32 | Problem | Unknown device config есть, но discovery отсутствует. |
| 33 | Improve | Показывать unknown devices в diagnostics. |
| 34 | Done | `ScrollEvent` нормализует vertical/horizontal delta. |
| 35 | Done | `ScrollEvent` содержит `continuous`. |
| 36 | Done | `ScrollEvent` содержит `synthetic`. |
| 37 | Done | `ScrollEvent` содержит `source_pid`. |
| 38 | Problem | `ScrollEvent` не содержит timestamp. |
| 39 | Improve | Добавить monotonic timestamp для diagnostics. |
| 40 | Problem | `ScrollEvent` не содержит event phase. |
| 41 | Improve | Добавить phase после gesture/HID spike. |
| 42 | Problem | `ScrollEvent` не содержит device id. |
| 43 | Improve | Добавить optional `device_id`. |
| 44 | Done | `scroll::transform_event` чисто тестируется. |
| 45 | Done | Disabled config делает pass-through. |
| 46 | Done | Synthetic event делает pass-through. |
| 47 | Done | Raw-input guard пропускает injected events. |
| 48 | Done | CLI simulate умеет задавать `source_pid`. |
| 49 | Improve | Добавить integration test для `simulate --source-pid`. |
| 50 | Done | CLI simulate умеет задавать `synthetic`. |
| 51 | Improve | Добавить integration test для `simulate --synthetic`. |
| 52 | Done | Vertical reverse включен по умолчанию. |
| 53 | Done | Horizontal reverse выключен по умолчанию. |
| 54 | Done | Mouse reverse включен по умолчанию. |
| 55 | Done | Trackpad reverse выключен по умолчанию. |
| 56 | Done | `reverse_magic_mouse` применяется live через `DeviceKind::MagicMouse`. |
| 57 | Done | UI показывает отдельный рабочий Magic Mouse checkbox. |
| 58 | Done | Step size применяется к non-continuous wheel delta. |
| 59 | Problem | Step size logic живет рядом с reverse logic. |
| 60 | Improve | Вынести wheel step в `scroll::wheel`. |
| 61 | Done | `discrete_scroll_step_size` валидируется диапазоном 0..=20. |
| 62 | Problem | Диапазон step size не объяснен в docs. |
| 63 | Improve | Добавить описание: 0 means system/default/no adjustment. |
| 64 | Done | `saturating_neg` предотвращает overflow. |
| 65 | Done | Step size multiplication использует `saturating_mul`. |
| 66 | Improve | Оставить regression test на будущий рост диапазона step size. |
| 67 | Done | CoreGraphics derived delta regression покрыт тестом. |
| 68 | Done | CoreGraphics helpers вынесены из `scroll.rs`; он теперь чистая политика. |
| 69 | Done | CGEvent field code живет в `platform/macos/scroll_events.rs`. |
| 70 | Done | Event tap disabled recovery re-enables через сохраненный |
| 71 | Problem | Event tap install не имеет integration smoke test. |
| 72 | Improve | Добавить mock listener для runtime contract tests. |
| 73 | Problem | `OnceLock<AppConfig>` делает event tap одноразовым в процессе. |
| 74 | Improve | Для будущего UI нужен runtime state с reloadable config snapshot. |
| 75 | Problem | Нет hot reload config. |
| 76 | Improve | Добавить command `reload` или runtime channel. |
| 77 | Problem | Нет pause без изменения config. |
| 78 | Done | Persistent `enabled` отделен от process-local temporary pause. |
| 79 | Done | CLI `enable`, `disable`, `toggle` меняют config. |
| 80 | Done | CLI commands теперь проходят через отдельный parser в `src/cli.rs`. |
| 81 | Improve | Для большего CLI все еще можно добавить `clap`, но текущий parser мал и покрыт тестами. |
| 82 | Done | `main.rs` больше не содержит ручной parsing flags для `simulate`. |
| 83 | Done | CLI parsing вынесен в отдельный command/options module. |
| 84 | Done | `parse_bool` принимает yes/no/1/0, и help теперь перечисляет эти значения. |
| 85 | Done | Help перечисляет accepted bool values: true/false/yes/no/1/0. |
| 86 | Problem | CLI errors не имеют stable error codes. |
| 87 | Improve | Добавить `E_CONFIG_PARSE`, `E_PERMISSION`, `E_PLATFORM`. |
| 88 | Done | `AppError` отделяет IO, config, platform и usage. |
| 89 | Problem | `AppError::InvalidConfig` хранит plain string. |
| 90 | Improve | Сделать structured validation errors. |
| 91 | Problem | `AppError::Platform` слишком общий. |
| 92 | Done | Tap lifecycle типизирован через `ui::runtime::State` и `TapRunOutcome`. |
| 93 | Done | Accessibility check реализован. |
| 94 | Done | Лишний Input Monitoring preflight удалён из runtime gate; Accessibility достаточно. |
| 95 | Done | Неиспользуемый `request_input_monitoring_access` удалён вместе с лишним permission gate. |
| 96 | Done | Missing Input Monitoring больше не блокирует запуск и не требует request action. |
| 97 | Done | Accessibility prompt вызывается через documented trusted options. |
| 98 | Done | Один `request_scroll_control_access` обслуживает CLI и GUI startup. |
| 99 | Done | `doctor` показывает exact current executable path. |
| 100 | Done | `doctor` печатает current executable path рядом с config path. |
| 101 | Done | `doctor --no-create` убирает config creation side effect. |
| 102 | Done | `doctor --no-create` и first-run `init` теперь разделены. |
| 103 | Done | `doctor` показывает единственное обязательное Accessibility permission. |
| 104 | Problem | `doctor` не проверяет event tap installability. |
| 105 | Improve | Добавить dry install check или explicit explanation. |
| 106 | Problem | Нет runtime diagnostics buffer. |
| 107 | Improve | Добавить ring buffer для последних decisions. |
| 108 | Problem | Event hot path не должен логировать синхронно. |
| 109 | Improve | Использовать lock-free/ring buffer или sampled logging. |
| 110 | Problem | Нет tracing/log crate. |
| 111 | Improve | Ввести `tracing` только после выбора diagnostics design. |
| 112 | Problem | Нет benchmark hot path. |
| 113 | Improve | Добавить microbenchmark для `transform_event`. |
| 114 | Problem | Нет property tests для sign reversal. |
| 115 | Improve | Проверить invariant: magnitude сохраняется кроме wheel step. |
| 116 | Problem | Нет теста для `i64::MIN` vertical/horizontal. |
| 117 | Improve | Добавить regression tests для saturating behavior. |
| 118 | Done | Есть тест для step size 0. |
| 119 | Improve | Добавить CLI simulation example для step size 0 после command support. |
| 120 | Problem | Нет теста для `reverse_unknown`. |
| 121 | Improve | Добавить unknown-device transform test. |
| 122 | Done | Pure transform покрывает Magic Mouse config. |
| 123 | Done | Live Magic Mouse distinction подключен через passive gesture adapter. |
| 124 | Done | Pure classifier contract отделен от AppKit и покрывает inventory/timing/momentum transitions. |
| 125 | Done | Inventory matrix, conservative fallback и gesture classifier покрыты unit tests. |
| 126 | Done | Device parse/display round-trip покрыт. |
| 127 | Problem | Нет serde round-trip для `DeviceKind`. |
| 128 | Improve | Добавить TOML test для `magic-mouse`. |
| 129 | Problem | Нет CLI snapshot tests. |
| 130 | Done | CLI integration tests запускают real binary через `std::process::Command` в sandbox. |
| 131 | Problem | Нет golden output для `show-config`. |
| 132 | Improve | Зафиксировать config output или сделать формат explicit unstable. |
| 133 | Problem | Нет test tempdir crate. |
| 134 | Improve | Использовать `tempfile` вместо timestamp path helper. |
| 135 | Problem | Tests оставляют файл, если panic до cleanup. |
| 136 | Improve | `tempfile::NamedTempFile` решит cleanup. |
| 137 | Problem | Нет module-level docs. |
| 138 | Improve | Добавить краткие `//!` docs для модулей. |
| 139 | Problem | Публичный API слишком широк: все modules `pub`. |
| 140 | Improve | Экспортировать facade, скрывать platform internals. |
| 141 | Problem | `event_tap` публичен из lib. |
| 142 | Improve | После UI/runtime split сделать platform modules crate-private. |
| 143 | Done | `permissions` переехал под platform-слой. |
| 144 | Done | Модуль живет в `src/platform/macos/permissions.rs`. |
| 145 | Problem | Проект пока macOS-only, но docs говорят о future cross-platform. |
| 146 | Done | `src/platform/mod.rs` cfg-gate'ит `macos`; бинарь дает понятный compile_error! вне macOS. |
| 147 | Problem | Non-macOS build behavior не определен. |
| 148 | Improve | Сделать graceful compile error или stub platform. |
| 149 | Problem | Cargo features не разделяют platform code. |
| 150 | Improve | Добавить feature `macos-event-tap`. |
| 151 | Done | `core-graphics`/`core-foundation` стали target-specific dependencies. |
| 152 | Done | Cargo.toml: `[target.'cfg(target_os = "macos")'.dependencies]`. |
| 153 | Problem | Нет MSRV. |
| 154 | Improve | Зафиксировать Rust version через `rust-toolchain.toml`. |
| 155 | Problem | Edition 2024 требует свежий toolchain. |
| 156 | Improve | README должен назвать required Rust version. |
| 157 | Done | Есть macOS GitHub Actions CI. |
| 158 | Done | CI запускает полный local quality gate и bundle smoke. |
| 159 | Problem | Нет `cargo audit`. |
| 160 | Improve | Добавить audit в release checklist. |
| 161 | Problem | Нет license. |
| 162 | Improve | Выбрать MIT/Apache-2.0/другую license до публикации. |
| 163 | Problem | Нет changelog. |
| 164 | Improve | Добавить `CHANGELOG.md` с текущим first slice. |
| 165 | Problem | Нет ADR. |
| 166 | Improve | Создать ADR for event tap, config format, CLI first. |
| 167 | Done | `.idea/` добавлен в root `.gitignore`. |
| 168 | Improve | IDE metadata остается локальным и не попадает в commit. |
| 169 | Done | Remote `origin` настроен. |
| 170 | Done | Push в `origin/master` работает. |
| 171 | Done | Menu bar app есть: native AppKit `NSStatusItem`. |
| 172 | Done | macOS status item стал primary always-on UI для Open Settings/Quit. |
| 173 | Done | CLI больше не единственный способ менять настройки. |
| 174 | Done | Preferences/settings window добавлен через egui. |
| 175 | Done | Missing permissions дают compact permission-first state без marketing welcome. |
| 176 | Done | Первый запуск выбирает Permissions и показывает targeted recovery actions. |
| 177 | Done | Visible active/paused/permission state есть в header окна и status-dot трея. |
| 178 | Done | Status icon отражает active, paused и permission-blocked через отдельную status-dot. |
| 179 | Done | Есть temporary pause на 15 минут. |
| 180 | Done | Pause process-local и не пишет config. |
| 181 | Problem | Right-click toggle не реализован. |
| 182 | Improve | Повторить Scroll Reverser: right/control click toggles app. |
| 183 | Done | Option-click debug console реализован через rich tray menu handling. |
| 184 | Done | Option-click открывает Debug Console; меню также имеет явный fallback пункт. |
| 185 | Problem | Hide menu bar icon config есть, UI нет. |
| 186 | Improve | Реализовать show/hide icon с recovery через CLI. |
| 187 | Done | Start at login config теперь связан с LaunchAgent integration. |
| 188 | Done | Packaged `.app` дополнен `SMAppService.mainAppService()` для GUI login item. |
| 189 | Problem | Update config fields есть, updater нет. |
| 190 | Improve | Решить: Sparkle, manual releases или no auto-update. |
| 191 | Problem | Beta updates flag есть, behavior нет. |
| 192 | Improve | Скрыть/пометить beta flag до update strategy. |
| 193 | Problem | `show_discrete_scroll_options` есть, UI нет. |
| 194 | Improve | Показывать wheel step section после wheel event. |
| 195 | Problem | Нет device list. |
| 196 | Improve | Settings first screen должен быть device-oriented. |
| 197 | Problem | Нет last active device. |
| 198 | Improve | Diagnostics should show last source and rule. |
| 199 | Problem | Нет device aliases. |
| 200 | Improve | Позволить переименовать устройства после registry. |
| 201 | Problem | Нет disconnected device state. |
| 202 | Improve | Показывать known/disconnected devices отдельно. |
| 203 | Done | Restore defaults реализован. |
| 204 | Done | Reset требует confirmation и показывает число device rules. |
| 205 | Problem | Нет undo для settings changes. |
| 206 | Improve | Добавить short undo toast для non-destructive changes. |
| 207 | Problem | Нет settings validation UI. |
| 208 | Improve | Ошибки config показывать рядом с полем. |
| 209 | Problem | Нет import/export config. |
| 210 | Improve | Export config для backup и support. |
| 211 | Problem | Import может принести invalid TOML. |
| 212 | Improve | Validate before applying imported config. |
| 213 | Done | Permissions panel имеет action button для открытия Privacy & Security. |
| 214 | Done | Permission UI открывает только обязательный Accessibility pane. |
| 215 | Done | Accessibility flow унифицирован между CLI, UI и tray. |
| 216 | Improve | Добавить OS-specific instructions. |
| 217 | Problem | Permission status только в CLI. |
| 218 | Improve | Показывать status badges in UI. |
| 219 | Problem | Нет state `Degraded`. |
| 220 | Improve | Runtime state: Active, Paused, NeedsPermission, Degraded, Error. |
| 221 | Done | Есть lightweight tap lifecycle runtime. |
| 222 | Done | `ui/runtime.rs` использует explicit channel events и typed state. |
| 223 | Problem | UI может напрямую дергать config store. |
| 224 | Improve | UI должен отправлять `AppCommand`. |
| 225 | Done | Design tokens вынесены из app coordinator. |
| 226 | Done | `ui/theme.rs` содержит handoff colors, spacing, radii и custom controls. |
| 227 | Problem | Product может стать слишком декоративным. |
| 228 | Improve | Использовать native compact utility layout. |
| 229 | Problem | Cards могут захламить настройки. |
| 230 | Improve | Использовать tables/lists вместо card grid. |
| 231 | Problem | Первый экран может стать landing page. |
| 232 | Improve | Первый экран должен быть рабочей панелью. |
| 233 | Problem | UI labels могут быть техническими. |
| 234 | Improve | Использовать понятные тексты: Mouse, Trackpad, Wheel step. |
| 235 | Problem | `Natural` не всем понятно. |
| 236 | Improve | Добавить microcopy: content moves with fingers vs opposite. |
| 237 | Problem | Слишком много helper text перегрузит UI. |
| 238 | Improve | Основные пояснения в tooltip/help popover. |
| 239 | Problem | Tooltips недоступны keyboard-only users. |
| 240 | Improve | Важные permission explanations держать inline. |
| 241 | Problem | Цветом нельзя единственным способом показывать статус. |
| 242 | Improve | Добавить labels/icons for state. |
| 243 | Problem | Нет accessibility labels. |
| 244 | Improve | Все controls должны иметь accessible names. |
| 245 | Problem | Нет keyboard navigation plan. |
| 246 | Improve | Tab order должен проходить все settings. |
| 247 | Problem | Нет dark mode QA. |
| 248 | Improve | Follow system appearance and test both themes. |
| 249 | Problem | Иконки могут не соответствовать macOS conventions. |
| 250 | Improve | Использовать native symbols или аккуратные monochrome assets. |
| 251 | Problem | Нет retina status icon review. |
| 252 | Improve | Проверить icon на light/dark menu bar. |
| 253 | Problem | Длинные device names ломают layout. |
| 254 | Improve | Truncate middle with tooltip. |
| 255 | Problem | Compact UI может обрезать русский текст. |
| 256 | Improve | Проверить localization expansion 30 percent. |
| 257 | Problem | Нет i18n structure. |
| 258 | Improve | Вынести strings до добавления второго языка. |
| 259 | Problem | README смешивает English и Russian. |
| 260 | Improve | Выбрать docs language или разделить localized docs. |
| 261 | Problem | Русский пользователь просит русскую документацию. |
| 262 | Improve | Добавить `README.ru.md` или перевести основной README. |
| 263 | Problem | Product name не закреплен визуально. |
| 264 | Improve | Settings title and about panel should say Auto Reverse. |
| 265 | Problem | Нет about panel. |
| 266 | Improve | About panel: version, config path, repo, privacy. |
| 267 | Problem | Нет privacy UX. |
| 268 | Improve | Сказать: no network telemetry by default. |
| 269 | Problem | Update checks могут противоречить privacy. |
| 270 | Improve | Automatic update checks only opt-in. |
| 271 | Problem | Debug console может показать sensitive data. |
| 272 | Improve | Log only scroll metadata, never text input. |
| 273 | Problem | Input hooks вызывают trust concerns. |
| 274 | Improve | UI должен объяснять, зачем нужны permissions. |
| 275 | Problem | Нет recovery when icon hidden. |
| 276 | Improve | CLI `show-icon` или relaunch opens preferences. |
| 277 | Problem | Нет `open-settings` CLI. |
| 278 | Improve | Добавить command to open preferences when UI exists. |
| 279 | Problem | Нет `doctor --json`. |
| 280 | Improve | JSON diagnostics помогут support. |
| 281 | Done | Debug Console экспортирует filtered structured events через native Save Panel. |
| 282 | Partial | CSV local-only и structured; отдельный redaction/config-snapshot режим еще открыт. |
| 283 | Problem | Нет copy-to-clipboard action. |
| 284 | Improve | Diagnostics UI: copy summary. |
| 285 | Problem | Нет manual test window. |
| 286 | Improve | Добавить scroll test area в debug console. |
| 287 | Problem | Test area может перехватить реальные expectations. |
| 288 | Improve | Clearly label it as simulation-only. |
| 289 | Problem | Нет visual preview of direction. |
| 290 | Improve | Small scroll preview can show content movement. |
| 291 | Problem | Preview animations могут отвлечь. |
| 292 | Improve | Keep animations minimal and disable-able. |
| 293 | Problem | Нет профилей. |
| 294 | Improve | Profiles можно отложить до real device registry. |
| 295 | Problem | App-specific rules слишком сложны. |
| 296 | Improve | Не делать app-specific rules до stable v1. |
| 297 | Problem | Нет quick reset for bad settings. |
| 298 | Improve | Add `auto-reverse reset-config`. |
| 299 | Problem | Reset может потерять useful config. |
| 300 | Improve | Reset should create backup first. |
| 301 | Problem | Нет clear disabled state in menu. |
| 302 | Improve | Disabled controls should show reason and re-enable action. |
| 303 | Problem | Нет separation persistent vs session settings. |
| 304 | Improve | Mark session-only controls clearly. |
| 305 | Problem | Нужен дизайн для error states. |
| 306 | Improve | Error rows: plain language, technical details hidden. |
| 307 | Problem | Нет loading states. |
| 308 | Improve | Device scan and permissions refresh need non-jumpy states. |
| 309 | Problem | Нет empty state. |
| 310 | Improve | If no devices, show permissions and "scroll to detect". |
| 311 | Problem | Нет menu hierarchy. |
| 312 | Improve | Menu: Enable, Preferences, Diagnostics, Quit. |
| 313 | Problem | Menu может стать слишком длинным. |
| 314 | Improve | Keep advanced actions inside preferences. |
| 315 | Problem | Нет keyboard shortcut policy. |
| 316 | Improve | Avoid global hotkey until conflicts are handled. |
| 317 | Problem | Нет native alerts strategy. |
| 318 | Improve | Use alerts only for destructive actions. |
| 319 | Problem | Нет onboarding completion state. |
| 320 | Improve | Store first-run flag separately from config rules. |
| 321 | Problem | Нет welcome copy. |
| 322 | Improve | Welcome: one sentence goal, two permission steps, open settings. |
| 323 | Problem | Нет visual hierarchy. |
| 324 | Improve | Status first, devices second, advanced third. |
| 325 | Problem | Нет responsive window sizing. |
| 326 | Improve | Define minimum width and resizable constraints. |
| 327 | Problem | Нет high-contrast review. |
| 328 | Improve | Test contrast in light/dark/high contrast modes. |
| 329 | Problem | Нет reduced motion support. |
| 330 | Improve | Honor reduce motion for preview animations. |
| 331 | Problem | Нет localization QA. |
| 332 | Improve | Test English/Russian strings in compact window. |
| 333 | Problem | Нет icon-only tooltip plan. |
| 334 | Improve | Every icon button needs tooltip. |
| 335 | Problem | Нет docs for hidden advanced flags. |
| 336 | Improve | `reverse_only_raw_input` needs docs and UI explanation. |
| 337 | Problem | Raw-input mode wording confusing. |
| 338 | Improve | Label it "Ignore injected/remote scroll events". |
| 339 | Problem | Нет support for restoring menu icon after hidden config mistake. |
| 340 | Improve | Document `show_menu_bar_icon = true` recovery. |
| 341 | Implemented | Local install/update/uninstall и production release pipeline готовы; реальный Developer ID/notary QA остаётся. |
| 342 | Done | Local app bundle structure выбран: `target/<profile>/Auto Reverse.app`. |
| 343 | Implemented | Developer ID signing path и strict authority gate готовы; сертификат является external prerequisite. |
| 344 | Done | Developer ID plan и release contract документированы. |
| 345 | Implemented | `notarytool`/stapler/Gatekeeper pipeline готов; реальная submission ждёт credentials. |
| 346 | Done | `RELEASE.md` содержит canonical notarization checklist. |
| 347 | Done | Есть atomic installer/updater и identity-checked uninstaller. |
| 348 | Done | Packaging включает debug/release bundle, stable install path, rollback и isolated workflow smoke. |
| 349 | Done | LaunchAgent implementation добавлен в `platform/macos/startup.rs`. |
| 350 | Done | Add `SMAppService` path when the app bundle exists. |
| 351 | Partial | Wake recovery реализован; реальный sleep/wake на собранном bundle еще не отмечен в QA. |
| 352 | Done | NSWorkspace DidWake проверяет permissions и re-arm/restart tap через bounded recovery. |
| 353 | Problem | Event tap can stop silently in edge cases. |
| 354 | Improve | Runtime health should detect no events/disabled tap. |
| 355 | Problem | Нет watchdog. |
| 356 | Improve | Add lightweight health timer after UI runtime exists. |
| 357 | Problem | Нет crash-safe state restoration. |
| 358 | Improve | Ensure failure path keeps pass-through behavior. |
| 359 | Problem | Panic in callback would be dangerous. |
| 360 | Improve | Keep callback small and panic-free; wrap risky code. |
| 361 | Problem | `toml::to_string_pretty` in save can fail but no recovery UX. |
| 362 | Improve | Surface config write errors in UI. |
| 363 | Done | Persistent `config.toml.lock` защищает cooperating config writers. |
| 364 | Done | `File::lock` сериализует CLI/UI/tray операции на одном lock inode. |
| 365 | Done | Exact TOML revision CAS отклоняет stale GUI/tray save вместо last-writer-wins. |
| 366 | Done | CLI использует locked `update`, GUI/tray - `save_if_unchanged` с reload конфликта. |
| 367 | Done | Базовый single-instance behavior есть: `run.lock` для tap и `ui.lock` для окна/tray. |
| 368 | Done | Relaunch publishes `ui.activate`; the owner reveals and focuses its existing window. |
| 369 | Problem | `OnceLock` blocks multiple install attempts in one process. |
| 370 | Improve | Runtime should own tap lifecycle explicitly. |
| 371 | Problem | Нет graceful shutdown tests. |
| 372 | Improve | Add shutdown path before UI. |
| 373 | Problem | Нет signal handling for CLI run. |
| 374 | Improve | Handle Ctrl+C gracefully. |
| 375 | Done | Manual QA checklist хранится в repo. |
| 376 | Done | Добавлен `QA.md` с automated/manual matrix. |
| 377 | Problem | Нет test matrix for devices. |
| 378 | Improve | Matrix: wheel mouse, Magic Mouse, built-in trackpad, Magic Trackpad. |
| 379 | Problem | Нет remote desktop test. |
| 380 | Improve | Test `reverse_only_raw_input` with injected source_pid. |
| 381 | Problem | Нет high-resolution wheel test. |
| 382 | Improve | Test fractional/pixel-like fields on real devices. |
| 383 | Problem | Нет horizontal wheel test. |
| 384 | Improve | Test tilt wheel and horizontal gestures. |
| 385 | Problem | Нет Wacom compatibility. |
| 386 | Improve | Document Wacom behavior after hardware test. |
| 387 | Problem | Нет accessibility-device review. |
| 388 | Improve | Avoid breaking assistive input devices. |
| 389 | Problem | Нет "shake to locate cursor" regression review. |
| 390 | Improve | Include macOS accessibility gestures in manual QA. |
| 391 | Problem | Нет Notification Center/gesture edge-case QA. |
| 392 | Improve | Test system gestures while tap is active. |
| 393 | Problem | Swipe gestures not reversed. |
| 394 | Improve | Document limitation prominently. |
| 395 | Problem | Custom scroll surfaces may bypass CGEvent. |
| 396 | Improve | Document app-specific limitations. |
| 397 | Problem | iPhone Mirroring-like cases may bypass transform. |
| 398 | Improve | Keep limitations list updated. |
| 399 | Problem | Нет source attribution in docs for Scroll Reverser parity. |
| 400 | Improve | Keep links in `scroll-reverser-parity.md`. |
| 401 | Problem | Нет legal review of feature parity wording. |
| 402 | Improve | Avoid implying affiliation with Scroll Reverser. |
| 403 | Problem | Нет release version policy. |
| 404 | Improve | Use SemVer after first tagged release. |
| 405 | Problem | Нет tag workflow. |
| 406 | Improve | Create release tags with changelog. |
| 407 | Problem | Нет build reproducibility notes. |
| 408 | Improve | Document toolchain and target. |
| 409 | Problem | Нет binary size budget. |
| 410 | Improve | Track size before adding GUI toolkit. |
| 411 | Problem | GUI toolkit may dominate app size. |
| 412 | Improve | Prefer native AppKit or small wrapper for macOS. |
| 413 | Problem | Cross-platform promise could overreach. |
| 414 | Improve | Market as macOS-first until adapters exist. |
| 415 | Problem | Linux/Windows support undefined. |
| 416 | Improve | Add future notes, not product promise. |
| 417 | Problem | Нет dependency policy. |
| 418 | Improve | Add dependencies only for clear use cases. |
| 419 | Done | Есть security policy. |
| 420 | Done | Добавлен `SECURITY.md` с FFI/release boundaries. |
| 421 | Done | Есть contribution guide. |
| 422 | Done | Добавлен `CONTRIBUTING.md` с full gate и layering rules. |
| 423 | Done | Есть issue template. |
| 424 | Done | Bug template собирает device/macOS/diagnostics без лишних personal данных. |
| 425 | Done | Есть privacy policy. |
| 426 | Done | `PRIVACY.md` фиксирует local-only data handling. |
| 427 | Problem | Update checks could send network requests. |
| 428 | Improve | Make network behavior explicit and opt-in. |
| 429 | Problem | Нет telemetry boundary tests. |
| 430 | Improve | Ensure no network crate enters default build without review. |
| 431 | Problem | Нет static analysis. |
| 432 | Improve | Run `cargo deny` or equivalent later. |
| 433 | Problem | Нет dependency license review. |
| 434 | Improve | Add license review to release checklist. |
| 435 | Problem | Нет localization pipeline. |
| 436 | Improve | Start with English and Russian string files. |
| 437 | Problem | Нет translation credit policy. |
| 438 | Improve | Track translator credits in changelog. |
| 439 | Problem | Нет screenshots. |
| 440 | Improve | Add real screenshots after UI exists. |
| 441 | Done | Есть template status glyph, colored state dot и branded app icon asset. |
| 442 | Done | Active/paused/temporary/permission icon states спроектированы. |
| 443 | Done | App icon добавлен. |
| 444 | Done | Bundle генерирует `.icns` до codesign. |
| 445 | Done | Design review artifact хранится в repo. |
| 446 | Done | Добавлен `DESIGN.md` с выбранным handoff и tokens. |
| 447 | Problem | Нет final product review process. |
| 448 | Improve | Review UX, reliability, privacy before each milestone. |
| 449 | Problem | Нет branch strategy. |
| 450 | Improve | Use `codex/` branch prefix for agent work. |
| 451 | Problem | Current work happened on `master`. |
| 452 | Improve | Next tasks should branch before larger changes. |
| 453 | Problem | Нет remote configured. |
| 454 | Improve | Add `origin` before expecting push. |
| 455 | Problem | Push cannot complete in current repo state. |
| 456 | Improve | User must provide repo URL or create remote. |
| 457 | Problem | Merge was local only. |
| 458 | Improve | Push merge commit after remote setup. |
| 459 | Done | `.idea/` is ignored at repository root. |
| 460 | Improve | Keep IDE metadata local unless the project intentionally standardizes IDE settings. |
| 461 | Done | `.gitignore` was reviewed after merge. |
| 462 | Improve | Later add patterns for generated release artifacts when packaging exists. |
| 463 | Problem | Docs use mixed Russian/English. |
| 464 | Improve | Split user docs by language. |
| 465 | Problem | README still says "target product" in English. |
| 466 | Improve | Translate README if primary user language is Russian. |
| 467 | Problem | Architecture doc is Russian-only. |
| 468 | Improve | Keep architecture Russian if it helps project learning. |
| 469 | Problem | Recommendation list can become stale quickly. |
| 470 | Improve | Refresh it after every milestone. |
| 471 | Done | 900-item audit больше не используется как ежедневная task queue. |
| 472 | Done | Есть `ROADMAP.md` с 25 P0/P1/P2 задачами. |
| 473 | Problem | Нет issue tracker mapping. |
| 474 | Improve | Convert top 20 recommendations to issues. |
| 475 | Problem | Нет owner per area. |
| 476 | Improve | Add ownership notes for config, platform, UI. |
| 477 | Done | Roadmap и QA задают acceptance criteria по типу задачи. |
| 478 | Done | Definition of Done требует tests/docs/manual check для visual changes. |
| 479 | Done | Definition of Done зафиксирован. |
| 480 | Done | `ROADMAP.md` определяет code/tests/docs/review/QA gate. |
| 481 | Problem | Нет automated review checklist. |
| 482 | Improve | Add checklist: bugs, risks, missing tests, UX, privacy. |
| 483 | Problem | Нет code review notes file. |
| 484 | Improve | Add `REVIEW.md` or keep review section in architecture. |
| 485 | Problem | Нет benchmark baseline. |
| 486 | Improve | Capture current transform performance. |
| 487 | Problem | Нет memory allocation audit. |
| 488 | Improve | Ensure hot path does not allocate. |
| 489 | Problem | Нет unsafe boundary documentation. |
| 490 | Improve | Document each FFI call and invariant. |
| 491 | Done | FFI permissions компилируются только под `#[cfg(target_os = "macos")]` через platform/mod.rs. |
| 492 | Done | Весь macOS FFI живет за cfg-gated `platform::macos`. |
| 493 | Problem | FFI function availability depends on macOS version. |
| 494 | Improve | Document minimum macOS version and fallback behavior. |
| 495 | Problem | No app-level state machine. |
| 496 | Improve | Add explicit state enum before UI. |
| 497 | Done | Final review fixes and updated docs are included in the merge/push commit. |
| 498 | Done | Full gate is required immediately before the final commit. |
| 499 | Done | Push destination exists today. |
| 500 | Done | Remote configured; `master` can be pushed with docs/review commits. |
<!-- TOP500_ARCHITECTURE:END -->

</details>

## Review Notes

Issues fixed after the latest merge:

- merge conflict in `src/config.rs` resolved without losing unique temp save IDs;
- stale docs updated to match current CLI/core reality;
- old "Hello, world" recommendations replaced with current audit.
- repeated review keeps exactly 500 base backlog items, later extended by N01-N400 through startup, menu bar, lifecycle, design, pause, icon, CI and documentation passes, then R01-R60 after the external research pass (total 960);
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
- 2026-07-04 second review fixed two lifecycle bugs: an immediate clean
  return from tap startup is no longer treated as success, and turning Reverse
  scrolling off clears the pending start attempt only when no tap thread is
  actually running, so a later enable can retry after failure without spawning
  redundant starts for a live pass-through tap.
- 2026-07-04 second review also added a GUI single-instance `ui.lock` and a
  save-error rollback, so a failed config save no longer leaves widgets showing
  values that the live tap did not adopt.
- 2026-07-09 final review mirrored the canonical base Top 500 into this file
  and README, then fixed two tray issues found during review: device action
  indexes now use `usize` instead of truncating through `u8`, and the status
  dot repaints when the menu-bar appearance changes between light/dark even if
  the app status itself stays the same.
- 2026-07-10 three-pass review split UI/runtime/theme/diagnostics and pure tray
  rules, added typed tap outcomes, lock-free temporary pause, permission-first
  recovery, reset confirmation, a branded `.icns`, CI/release docs, and bundle
  smoke. The final adversarial pass fixed Starting-disable lifecycle loss,
  pause-vs-permission priority, reset lifecycle recomputation, and handoff glyph
  regressions.
- 2026-07-11 install workflow review added stable release install/update,
  identity-checked uninstall and CI smoke. Its adversarial pass fixed damaged
  bundle repair, fresh-install rollback cleanup, broad process matching and
  unchecked SMAppService cleanup.
- 2026-07-12 release review added hardened local signing, strict Developer ID
  and timestamp gates, Keychain-only notary authentication, Accepted-status
  enforcement, audit-log capture, stapling, Gatekeeper assessment and final
  checksum. Its adversarial pass fixed deletion of a previous good ZIP before
  a replacement had passed notarization and prohibited identity-only validation
  from being combined with strict release/notarization flags.

Known risks still open:

- menu bar UI is now rich enough for daily use, but still needs real Retina
  and multi-appearance manual QA for every icon/menu state;
- Magic Mouse classification is implemented, but physical rapid-alternation QA remains;
- no config migration yet;
- the production pipeline is implemented, but this Mac has no Developer ID
  Application certificate; real notarization, stapling, quarantined
  clean-machine Gatekeeper and TCC continuity are still unverified.
