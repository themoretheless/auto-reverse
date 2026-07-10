# Auto Reverse

Auto Reverse is a Rust/macOS utility for reverse scrolling. The target product is feature parity with Scroll Reverser: keep a trackpad natural, reverse a wheel mouse, expose clear settings, and never make system input feel mysterious.

## Current Status

Implemented:

- macOS `CGEventTap` for scroll events;
- TOML config with `config_version`;
- global enable/disable;
- vertical and horizontal reverse flags;
- mouse, trackpad, Magic Mouse and unknown-device config flags;
- wheel step size;
- raw-input guard through `source_pid`;
- Accessibility and Input Monitoring checks;
- local macOS `.app` bundle for Privacy & Security;
- LaunchAgent start at login via `enable-startup`/`disable-startup`;
- per-device rules: `[[device_rules]]` pins one exact mouse (vendor/product
  ID) on or off, attributed via an IOHIDManager wheel monitor; `devices`
  lists connected pointing devices with their IDs;
- egui settings window (`ui`, default `gui` feature); opening it starts the
  scroll event tap in the same process when enabled and permissions are ready,
  deduped against any other already-running tap via `daemon_lock`;
- menu bar UI with a custom opposing-arrows template icon, a separate colored
  status dot, a rich native menu, a Reverse Scrolling toggle, per-device
  quick-pick submenu, temporary pause/resume, Open Settings, Open Debug Console,
  and Quit;
- local Debug Console with search, decision filters, export, clear, and an
  in-memory ring buffer of recent scroll decisions;
- process-local 15-minute pause that leaves persisted settings untouched;
- typed event-tap lifecycle with explicit started/already-running/stopped/failed
  events rather than timeout-inferred booleans;
- permission-first initial tab, targeted Accessibility/Input Monitoring actions,
  and confirmation before Restore defaults removes per-device rules;
- branded opposing-arrows app icon and generated Retina `.icns` in the bundle;
- GUI Start at Login toggle via `SMAppService.mainAppService()`;
- CLI diagnostics, JSON startup status and simulation;
- separated CLI parser in `src/cli.rs`;
- macOS CI plus roadmap, design, QA, privacy, security, and contribution docs.

Still missing:

- guided onboarding beyond the compact permission-first state;
- hide/show menu bar icon;
- gesture/HID classifier for Magic Mouse vs trackpad;
- packaging/signing/update flow.

## Commands

```bash
cargo build
scripts/build-app-bundle.sh
cargo run -- doctor
cargo run -- doctor --no-create
cargo run -- devices
cargo run -- ui
cargo run -- show-config
cargo run -- simulate --device mouse --dy 1 --dx 2 --continuous false
cargo run -- simulate --device mouse --dy 1 --vendor-id 0x046d --product-id 0xc54d
cargo run -- enable
cargo run -- disable
cargo run -- toggle
cargo run -- enable-startup
cargo run -- disable-startup
cargo run -- startup-status
cargo run -- startup-status --json
cargo run -- run
cargo test
cargo fmt
cargo clippy -- -D warnings
scripts/check-app-bundle.sh
```

`run` installs the macOS event tap. It requires:

- System Settings -> Privacy & Security -> Accessibility;
- System Settings -> Privacy & Security -> Input Monitoring.

For safe checks without installing the event tap, use `doctor`, `startup-status`, `show-config`, and `simulate`. `doctor --no-create` reports defaults without creating the config file.

## App Bundle

Build a local bundle:

```bash
scripts/build-app-bundle.sh
```

Default output:

```text
target/debug/Auto Reverse.app
```

The bundle contains `Contents/Resources/AutoReverse.icns`; the build script
generates its complete Retina iconset from `assets/AppIcon.svg`, ad-hoc signs
the local build, and validates the Mach-O/plist/icon/signature structure.

Use that `.app` in macOS:

- System Settings -> Privacy & Security -> Accessibility -> add `target/debug/Auto Reverse.app`;
- System Settings -> Privacy & Security -> Input Monitoring -> add `target/debug/Auto Reverse.app`.

Then launch the bundled app:

```bash
open "target/debug/Auto Reverse.app"
```

Double-clicking the bundle opens the settings window (`ui`), which also starts the scroll event tap on a background thread in this same process when `enabled=true` in the config and both permissions are granted, sharing one live config with the window so changes made in that window apply immediately with no restart. If the app was opened before permissions were granted, it keeps watching the permission state and retries starting the tap once both checks become ready; if startup failed or stopped immediately, turning Reverse scrolling off clears that pending attempt so turning it on again can retry cleanly. A menu-bar icon stays up for as long as the process runs: it uses an opposing-arrows template glyph plus a separate colored status dot for active/paused/permission-blocked states. Its native menu includes Reverse Scrolling, device quick-picks, Open Settings, Open Debug Console, and Quit; holding Option while opening the icon opens the Debug Console directly. Closing the settings window hides it rather than quitting. A separate `ui.lock` prevents duplicate windows/menu-bar icons, and an exclusive tap lock (`platform::macos::daemon_lock`) still guards tap installation, so this in-process tap and a separately started `run` (manual, or via a LaunchAgent) can never both hold a live event tap - whichever gets there first wins, and the other observes the lock held and does nothing. External CLI edits made while the settings window is already open do not update that running window; use the window itself, or quit and reopen it. For terminal diagnostics through the bundled identity:

The bundle uses the real Mach-O binary as `CFBundleExecutable`
(`Contents/MacOS/auto-reverse`) rather than a shell launcher. With no
arguments, that binary detects it is running inside `.app` and opens `ui`;
explicit CLI arguments still work through the same bundled executable.

```bash
"target/debug/Auto Reverse.app/Contents/MacOS/auto-reverse" doctor --no-create
```

## Config

`run` actually triggers both OS consent dialogs now (`AXIsProcessTrustedWithOptions` for Accessibility, `CGRequestListenEventAccess` for Input Monitoring), not just passive checks; `doctor` reports both permission states without prompting and prints the fix instructions when something is missing. An earlier experimental `SourceClassifier` (a touch-count/phase heuristic meant to separate Magic Mouse from trackpad) was removed as dead code: it was never wired into the real event tap (nothing in the codebase feeds it real touch data), and its own passing tests created false confidence that the distinction already worked. See `recommendation.md` for the full list of verified findings and fixes across 3 review iterations.

Default path on macOS:

```text
~/Library/Application Support/Auto Reverse/config.toml
```

Override path for testing:

```bash
AUTO_REVERSE_CONFIG=/tmp/auto-reverse.toml cargo run -- doctor
```

Important fields:

```toml
config_version = 1
enabled = true
reverse_vertical = true
reverse_horizontal = false
reverse_mouse = true
reverse_trackpad = false
reverse_magic_mouse = true
reverse_unknown = false
discrete_scroll_step_size = 3
reverse_only_raw_input = false

# Optional: pin one exact device regardless of the per-kind flags above.
# Run `auto-reverse devices` to see YOUR devices' IDs and paste them here -
# the values below are placeholders, not real hardware. Discrete wheels
# only; trackpad and Magic Mouse continuous scrolling cannot be attributed.
[[device_rules]]
vendor_id = 0x1234       # from `auto-reverse devices`
product_id = 0x5678      # from `auto-reverse devices`
name = "My mouse"        # optional, display only
reverse = false
```

Current limitation: `reverse_magic_mouse` is present for parity, but the live classifier cannot distinguish Magic Mouse from trackpad yet because both report continuous scroll through the current public event-tap signal.

## Start At Login

There are two start-at-login mechanisms because there are two launch styles:

- the GUI settings window uses `SMAppService.mainAppService()` and registers the `.app` bundle itself;
- the CLI command `enable-startup` installs a per-user LaunchAgent for the current binary path.

The CLI LaunchAgent lives at:

```text
~/Library/LaunchAgents/com.auto-reverse.agent.plist
```

That agent starts the current executable with the `run` argument on the next login. Use it for terminal/no-GUI installs. Use the settings window's Start at Login toggle for the bundled `.app`.

`startup-status --json` prints the LaunchAgent state, config path, config `start_at_login` value, and whether both are in sync. It does not create a config file just to report status.

## Architecture

Current modules, layered from pure logic down to platform code. Read them
top to bottom to learn the project in small pieces - each file has one
reason to change, and nothing above `platform/` imports an OS framework:

```text
src/main.rs                          CLI entrypoint and command orchestration
src/cli.rs                           command/flag parser and CLI option structs
src/lib.rs                           library facade documenting the layering
src/error.rs                         shared AppError / AppResult
src/device.rs                        DeviceKind + conservative classifier
src/input.rs                         normalized ScrollEvent
src/runtime.rs                       lock-free process-local pause control
src/scroll.rs                        pure reversal policy (no CoreGraphics)
src/config/mod.rs                    facade re-exporting AppConfig/ConfigStore
src/config/schema.rs                 what the settings ARE: fields, defaults, validation
src/config/store.rs                  where they LIVE: paths, TOML I/O, atomic save
src/platform/mod.rs                  cfg-gated platform adapters
src/platform/macos/mod.rs            macOS integration overview
src/platform/macos/scroll_events.rs  CGEvent field mapping (read event, write decision)
src/platform/macos/permissions.rs    Accessibility + Input Monitoring TCC calls
src/platform/macos/hid.rs            IOHIDManager wheel monitor (per-device attribution)
src/platform/macos/startup.rs        LaunchAgent start-at-login support (headless `run`)
src/platform/macos/event_tap.rs      CGEventTap runtime loop, config shared via Arc<RwLock<_>>
src/platform/macos/daemon_lock.rs    flock: only one live CGEventTap at a time, any launch path
src/platform/macos/debug_log.rs      local ring buffer for the Debug Console (gui feature only)
src/platform/macos/quit_handler.rs   AppleEvent quit interception so only tray Quit exits
src/platform/macos/login_item.rs     SMAppService.mainAppService() wrapper (gui feature only)
src/platform/macos/tray.rs           rich native menu-bar tray icon/menu (gui feature only)
src/platform/macos/tray/device_rules.rs pure tray quick-pick rule mutation
src/ui.rs                            settings app coordinator and tab contents
src/ui/runtime.rs                    typed tap lifecycle and explicit event channel
src/ui/theme.rs                      handoff tokens and custom egui controls
src/ui/debug_console.rs              Debug Console viewport/filter/export
```

The macOS framework crates (`core-foundation`, `core-graphics`) are
target-specific dependencies: the pure core compiles without them.

The remaining useful split is narrower: keep the AppKit icon/menu adapter in
`tray.rs`, move structured diagnostics behind a small sink, and split settings
tabs only when their behavior grows enough to justify another boundary.

## Three Iterations

### Iteration 1: Core Safety

Keep CLI and pure logic solid:

- finish config validation and migration plan;
- add more simulation flags;
- separate platform helpers from pure transform;
- keep tests fast and deterministic.

### Iteration 2: Product UX

Build the app surface:

- menu bar utility;
- settings window;
- permission onboarding;
- debug console and its dedicated module;
- process-local pause and permission-first recovery;
- hide/show icon.

### Iteration 3: Release Quality

Make it releasable:

- Magic Mouse/trackpad distinction;
- wake recovery;
- packaging/signing;
- localization;
- update strategy;
- privacy/security review.

## Design Direction

This should feel like a compact native utility:

- no landing page;
- no decorative gradients;
- dense but calm settings;
- icon buttons for common actions;
- visible permission states;
- system dark/light mode;
- clear recovery actions for errors.

## Top 500

The full working backlog lives in `recommendation.md`: 500 base findings plus
`N01-N400` follow-ups. The collapsed mirror below keeps the requested 500 base
items visible from the README without making the first read impossible.

<details>
<summary>Show 500 base recommendations, problems and improvements</summary>

<!-- TOP500_README:START -->
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
| 23 | Problem | Magic Mouse пока не определяется live classifier. |
| 24 | Improve | Добавить отдельный gesture/HID classifier. |
| 25 | Problem | Continuous scroll сейчас консервативно считается trackpad. |
| 26 | Improve | Явно показывать этот gap в CLI и UI. |
| 27 | Done | Non-continuous scroll считается mouse. |
| 28 | Problem | Нет stable device id. |
| 29 | Improve | Добавить `DeviceId` и `DeviceInfo`. |
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
| 56 | Problem | Magic Mouse reverse включен в config, но live classifier не умеет его применить. |
| 57 | Improve | Временно пометить `reverse_magic_mouse` как planned в docs/UI. |
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
| 78 | Done | Persistent enabled отделен от process-local temporary pause. |
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
| 92 | Done | Tap lifecycle типизирован через runtime state и TapRunOutcome. |
| 93 | Done | Accessibility check реализован. |
| 94 | Done | Input Monitoring preflight реализован. |
| 95 | Problem | `request_input_monitoring_access` не используется в CLI flow. |
| 96 | Improve | При missing Input Monitoring предлагать request action. |
| 97 | Problem | Accessibility prompt не вызывается через trusted options. |
| 98 | Improve | Добавить API для request Accessibility permission. |
| 99 | Done | `doctor` показывает exact current executable path. |
| 100 | Done | `doctor` печатает current executable path рядом с config path. |
| 101 | Done | `doctor --no-create` убирает config creation side effect. |
| 102 | Done | `doctor --no-create` и first-run `init` теперь разделены. |
| 103 | Done | `doctor` показывает Accessibility и Input Monitoring. |
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
| 123 | Improve | Live Magic Mouse distinction все еще требует gesture/HID classifier. |
| 124 | Problem | Live classifier не покрыт integration contract. |
| 125 | Improve | Добавить tests для `conservative_kind_from_continuity`. |
| 126 | Done | Device parse/display round-trip покрыт. |
| 127 | Problem | Нет serde round-trip для `DeviceKind`. |
| 128 | Improve | Добавить TOML test для `magic-mouse`. |
| 129 | Problem | Нет CLI snapshot tests. |
| 130 | Improve | Добавить integration tests через `assert_cmd`. |
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
| 158 | Done | CI запускает полный quality gate и bundle smoke. |
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
| 175 | Done | Missing permissions дают compact permission-first state. |
| 176 | Done | Первый запуск выбирает Permissions и targeted recovery actions. |
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
| 214 | Improve | Later split buttons: Request Input Monitoring, Open Accessibility Settings. |
| 215 | Problem | Accessibility request flow сложнее Input Monitoring. |
| 216 | Improve | Добавить OS-specific instructions. |
| 217 | Problem | Permission status только в CLI. |
| 218 | Improve | Показывать status badges in UI. |
| 219 | Problem | Нет state `Degraded`. |
| 220 | Improve | Runtime state: Active, Paused, NeedsPermission, Degraded, Error. |
| 221 | Done | Есть lightweight tap lifecycle runtime. |
| 222 | Done | `ui/runtime.rs` использует channel events и typed state. |
| 223 | Problem | UI может напрямую дергать config store. |
| 224 | Improve | UI должен отправлять `AppCommand`. |
| 225 | Done | Design tokens вынесены из app coordinator. |
| 226 | Done | `ui/theme.rs` хранит handoff colors, spacing, radii и controls. |
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
| 281 | Problem | Нет diagnostics export. |
| 282 | Improve | Export redacted diagnostics file. |
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
| 341 | Improve | Release packaging все еще не готов, но local dev `.app` bundle уже есть. |
| 342 | Done | Local app bundle structure выбран: `target/<profile>/Auto Reverse.app`. |
| 343 | Problem | Нет code signing. |
| 344 | Improve | Plan Developer ID signing before public release. |
| 345 | Problem | Нет notarization. |
| 346 | Improve | Add notarization checklist. |
| 347 | Problem | Нет installer/uninstaller. |
| 348 | Done | Первый шаг packaging сделан: headless drag-and-run `.app` для Privacy & Security. |
| 349 | Done | LaunchAgent implementation добавлен в `platform/macos/startup.rs`. |
| 350 | Done | Add `SMAppService` path when the app bundle exists. |
| 351 | Problem | Нет wake-from-sleep recovery. |
| 352 | Improve | Observe wake notifications and re-arm tap or relaunch. |
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
| 363 | Problem | Нет config lock. |
| 364 | Improve | Consider file lock if multiple CLI/UI instances write config. |
| 365 | Problem | Last-writer-wins может терять settings. |
| 366 | Improve | Runtime should serialize config writes. |
| 367 | Done | Базовый single-instance behavior есть: `run.lock` для tap и `ui.lock` для окна/tray. |
| 368 | Improve | Relaunch should focus existing settings window, not just fail on `ui.lock`. |
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
| 422 | Done | `CONTRIBUTING.md` фиксирует full gate и layering. |
| 423 | Done | Есть privacy-aware bug issue template. |
| 424 | Done | Bug template собирает macOS/device/diagnostics fields. |
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
| 441 | Done | Есть template glyph, colored state dot и branded app icon. |
| 442 | Done | Active/paused/temporary/permission icon states спроектированы. |
| 443 | Done | App icon добавлен. |
| 444 | Done | Bundle генерирует `.icns` до codesign. |
| 445 | Done | Design review artifact хранится в repo. |
| 446 | Done | Добавлен `DESIGN.md` с handoff и tokens. |
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
| 471 | Done | 900-item audit больше не daily task queue. |
| 472 | Done | Есть `ROADMAP.md` с 25 P0/P1/P2 задачами. |
| 473 | Problem | Нет issue tracker mapping. |
| 474 | Improve | Convert top 20 recommendations to issues. |
| 475 | Problem | Нет owner per area. |
| 476 | Improve | Add ownership notes for config, platform, UI. |
| 477 | Done | Roadmap и QA задают acceptance criteria. |
| 478 | Done | Definition of Done требует tests/docs/manual check. |
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
<!-- TOP500_README:END -->

</details>

## Documents

- `architecture.md` - current and target architecture, SOLID/DRY split, UX direction.
- `recommendation.md` - 900 recommendations, problems and improvements (500 base items + N01-N400 follow-ups through the SOLID/DRY, pause, app-icon, CI, and documentation pass).
- `ROADMAP.md` - the executable top 25, grouped P0/P1/P2.
- `DESIGN.md`, `QA.md`, `PRIVACY.md`, `SECURITY.md`, `CONTRIBUTING.md` - focused product and release contracts.
- `scroll-reverser-parity.md` - Scroll Reverser feature parity checklist.
