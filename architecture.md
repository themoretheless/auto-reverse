# Архитектура Auto Reverse

Документ описывает, как разложить свежий Rust binary crate в понятную программу для reverse scrolling. Сейчас проект находится в состоянии `cargo new`: в `src/main.rs` только `Hello, world!`, поэтому ниже не фиксация текущей реализации, а целевая архитектура, дизайн-решения и 3 итерации разработки.

Файл называется `architecture.md`, чтобы имя совпадало с общепринятым написанием.

## Цель продукта

Auto Reverse — маленькая системная утилита, которая автоматически меняет направление скролла по правилам пользователя. Базовый сценарий: трекпад остается в natural scrolling, а внешняя мышь получает обратное направление, либо наоборот.

Целевая функциональность: повторить возможности Scroll Reverser для macOS как feature parity, но реализовать их своим кодом и с Rust-архитектурой. Подробный чеклист лежит в `scroll-reverser-parity.md`.

Хороший результат:

- пользователь понимает состояние программы за 3 секунды;
- настройка занимает меньше минуты;
- программа не ломает системный ввод;
- каждое устройство можно настроить отдельно;
- есть понятный аварийный выход;
- код разложен маленькими модулями по SOLID и DRY.

## Feature parity со Scroll Reverser

Auto Reverse должен покрыть основные фичи Scroll Reverser:

- глобальное включение и выключение reverse scrolling;
- независимые настройки для mouse, trackpad и Magic Mouse;
- отдельные toggles для vertical и horizontal scrolling;
- step size control для scroll wheel;
- автоматический показ step size только после wheel/non-continuous scroll;
- menu bar/tray utility вместо большого постоянного окна;
- settings window с разделами `Scrolling`, `App`, `Permissions`;
- first-run welcome/onboarding;
- статусы Accessibility/Input Monitoring permissions;
- кнопки request/open system permission settings;
- start at login;
- show/hide menu bar icon;
- right-click/control-click по иконке для быстрого toggle;
- Option-click по иконке для debug console;
- efficient debug log без тяжелого логирования в hot path;
- relaunch/recover после wake from sleep;
- remote desktop/raw input mode;
- AppleScript/automation equivalent для включения и выключения;
- update flow: check now, automatic checks, beta updates;
- dark mode, native icon/status icon, аккуратный system UI;
- локализация, включая русский язык;
- честно задокументированные ограничения: swipe gestures, custom gesture scrolling, Calendar/iPhone Mirroring-like cases.

Это не означает копирование реализации Scroll Reverser. Это означает совместимый пользовательский набор возможностей, а внутренние модули остаются разделенными на domain, platform, runtime и UI.

## Что сейчас сделано плохо

Текущее состояние не ошибочное для scaffold, но для реальной утилиты почти все отсутствует:

- нет описания цели и сценариев;
- нет модели настроек;
- нет слоя платформы;
- нет обработки input events;
- нет device detection;
- нет разделения ответственности;
- нет тестов;
- нет UX-модели;
- нет стратегии прав доступа;
- нет логирования и диагностики;
- нет packaging/release-плана.

## Три итерации

### Итерация 1: минимальное ядро

Цель: доказать, что программа может безопасно читать настройки, определять устройство и принимать решение о направлении скролла.

Состав:

- CLI-запуск без GUI;
- конфиг в локальном файле;
- доменная модель `ScrollDirection`;
- правила для устройств;
- trait для платформенного adapter;
- mock adapter для тестов;
- unit tests для правил;
- parity checklist из `scroll-reverser-parity.md` подключен к roadmap;
- понятные ошибки через единый `AppError`.

Критерий готовности: можно запустить `cargo test`, увидеть покрытие логики правил и выполнить dry-run без системного перехвата ввода.

### Итерация 2: пользовательский продукт

Цель: сделать программу понятной обычному пользователю.

Состав:

- tray/menu bar состояние;
- экран настроек;
- список устройств;
- переключатели направления;
- onboarding по правам доступа;
- быстрый disable/pause;
- step size для wheel mouse;
- hide/show tray icon;
- start at login;
- debug console;
- профили: default, mouse, trackpad;
- локальные логи для диагностики.

Критерий готовности: пользователь может включить/выключить reverse scroll без чтения документации.

### Итерация 3: надежность и релиз

Цель: довести утилиту до состояния, где ей можно доверять каждый день.

Состав:

- integration tests;
- property-based tests для правил;
- стресс-тест событий;
- macOS/Windows/Linux packaging;
- migration для конфигов;
- crash recovery;
- versioned config schema;
- release checklist;
- privacy/security review.
- localization/update/release flow.

Критерий готовности: есть стабильный build, инструкции установки, тесты и понятная диагностика проблем.

## Предлагаемая структура

```text
src/
  main.rs
  app/
    mod.rs
    runtime.rs
    lifecycle.rs
  config/
    mod.rs
    schema.rs
    store.rs
    migration.rs
  device/
    mod.rs
    id.rs
    classifier.rs
    registry.rs
  input/
    mod.rs
    event.rs
    listener.rs
    normalizer.rs
  scroll/
    mod.rs
    direction.rs
    transformer.rs
    rules.rs
  platform/
    mod.rs
    macos.rs
    windows.rs
    linux.rs
  ui/
    mod.rs
    tray.rs
    settings.rs
    theme.rs
  telemetry/
    mod.rs
    logging.rs
  error.rs
tests/
  rules_tests.rs
  config_tests.rs
  device_tests.rs
```

## SOLID-разделение

### Single Responsibility

Каждый модуль должен иметь одну причину для изменения:

- `config` меняется из-за формата настроек;
- `device` меняется из-за определения устройств;
- `input` меняется из-за получения событий;
- `scroll` меняется из-за правил преобразования;
- `platform` меняется из-за системных API;
- `ui` меняется из-за интерфейса;
- `telemetry` меняется из-за логирования.

### Open/Closed

Новые платформы, правила и UI-оболочки добавляются через traits и новые implementations, а не через переписывание ядра.

Пример:

```rust
pub trait PlatformInput {
    fn listen(&mut self, sink: &mut dyn InputSink) -> Result<(), AppError>;
}
```

### Liskov Substitution

`MockPlatformInput`, `MacOsInput`, `WindowsInput` и `LinuxInput` должны быть взаимозаменяемы на уровне runtime. Тесты обязаны проверять одну и ту же контрактную логику для mock и реального adapter там, где это возможно.

### Interface Segregation

Не нужно делать один большой trait `Platform`. Лучше маленькие interfaces:

- `DeviceProvider`;
- `InputListener`;
- `ScrollEmitter`;
- `PermissionChecker`;
- `StartupInstaller`.

### Dependency Inversion

Core-логика не должна импортировать `macos.rs`, `windows.rs` или GUI. Направление зависимости:

```text
UI / CLI / Platform -> App Runtime -> Domain Rules
```

Домен ничего не знает о конкретной ОС.

## DRY-источники правды

Один источник правды нужен для:

- схемы конфига;
- списка поддержанных направлений;
- device id normalization;
- названий профилей;
- текстов ошибок;
- дизайн-токенов UI;
- release version;
- permission state;
- logging categories;
- keyboard shortcuts.

Если одна и та же строка или enum появляются в 3 местах, это почти всегда будущая ошибка.

## Доменная модель

Минимальные типы:

```rust
pub enum ScrollDirection {
    Natural,
    Reversed,
}

pub enum DeviceKind {
    Mouse,
    Trackpad,
    Unknown,
}

pub struct DeviceId(String);

pub struct ScrollRule {
    pub device_match: DeviceMatch,
    pub direction: ScrollDirection,
    pub enabled: bool,
}
```

Важная мысль: направление скролла — это доменное решение, а не UI checkbox. UI только меняет конфиг.

## Runtime-поток

```text
start
  -> load config
  -> check permissions
  -> discover devices
  -> start input listener
  -> normalize event
  -> resolve matching rule
  -> transform scroll delta
  -> emit event
  -> log decision if debug mode
```

## UX-дизайн

Главный интерфейс должен быть не лендингом, а рабочей поверхностью:

- верхняя строка: статус `Active`, `Paused`, `Needs Permission`;
- список устройств: имя, тип, последнее событие, направление;
- быстрый переключатель для каждого устройства;
- глобальный pause;
- кнопка диагностики;
- короткие состояния ошибок без технической стены текста;
- спокойная системная палитра, без декоративного шума;
- на macOS — menu bar utility, на Windows/Linux — tray app.

## Визуальный язык

Продукт системный, поэтому дизайн должен быть тихим и точным:

- компактная плотность информации;
- четкая иерархия;
- neutral background;
- один accent color для активного состояния;
- предупреждения только там, где есть риск;
- icons для pause, settings, diagnostics;
- без огромных hero-блоков;
- без маркетинговых карточек;
- без декоративных градиентов.

## Маленькие кусочки для изучения

Рекомендуемый порядок разработки:

1. `scroll::direction` — enum и тесты.
2. `scroll::rules` — выбор правила.
3. `device::id` — нормализация ID.
4. `config::schema` — структура настроек.
5. `config::store` — чтение/запись.
6. `input::event` — единая модель события.
7. `scroll::transformer` — изменение delta.
8. `platform::mod` — traits.
9. `platform::<os>` — реальная интеграция.
10. `app::runtime` — сборка всех частей.
11. `ui::tray` — быстрый контроль.
12. `ui::settings` — полноценные настройки.

Так проект можно читать слоями, не прыгая между системными API, GUI и бизнес-логикой.

## Минимальный backlog

- Создать README с ясной целью.
- Добавить `AppError`.
- Создать domain modules.
- Добавить config schema.
- Написать первые unit tests.
- Добавить mock platform.
- Реализовать dry-run режим.
- Потом уже подключать реальные OS API.
