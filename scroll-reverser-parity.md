# Scroll Reverser Feature Parity

Этот документ фиксирует фичи Scroll Reverser, которые Auto Reverse должен повторить как пользовательский набор возможностей. Источники: официальная страница Scroll Reverser, GitHub README, release history и просмотр публичного кода проекта.

Важно: цель — feature parity, а не копирование реализации. Auto Reverse должен повторять поведение и UX-ценность, но писать собственную Rust-архитектуру.

## Источники

- [Official home page](https://pilotmoon.com/scrollreverser/)
- [GitHub repository](https://github.com/pilotmoon/Scroll-Reverser)
- [Scroll Reverser `MouseTap.m` at the audited commit](https://github.com/pilotmoon/Scroll-Reverser/blob/187bf3945b6107cd8486327c6165f32e523535a4/MouseTap.m)
- [Apple `NSEvent`](https://developer.apple.com/documentation/appkit/nsevent)
- [Quartz Event Services](https://developer.apple.com/documentation/coregraphics/quartz_event_services)

## P0: обязательные фичи

- Reverse scrolling как главная функция.
- Глобальный toggle `Enable Auto Reverse`.
- Независимое включение reverse для mouse.
- Независимое включение reverse для trackpad.
- Поддержка Magic Mouse как mouse-like устройства.
- Отдельный toggle `Reverse Vertical`.
- Отдельный toggle `Reverse Horizontal`.
- По умолчанию не ломать системный ввод, если app disabled.
- Рекомендованный сценарий: системный natural scrolling включен, trackpad не reversed, mouse reversed.
- Нормализованная модель scroll event с `delta_x`, `delta_y`, timestamp, device kind и source flags.
- Event tap/input listener слой для scroll events.
- Gesture/input слой для определения trackpad vs mouse.
- Классификация trackpad через gesture-сигнал, когда доступны два или больше пальца.
- Safe fallback к trackpad policy, если passive gesture tap не установился;
  внутри активной momentum-сессии сохранять последний continuous source.
- Защита от повторного reverse synthetic events.
- Pass-through mode при ошибке permissions или platform hook.
- Permissions model для Accessibility.
- Permissions model для Input Monitoring.
- UI-статус permissions: granted/required.
- Action `Request permission`.
- Action `Open permission settings`.
- Settings window.
- Settings section `Scrolling`.
- Settings section `App`.
- Settings section `Permissions`.
- First-run welcome window/onboarding.
- Menu bar utility на macOS.
- Tray/status utility equivalent на других платформах.
- Menu item `Preferences`.
- Menu item `Quit`.
- Right-click/control-click по menu bar icon для быстрого enable/disable.
- Option-click по menu bar icon для debug console.
- Debug console/window для fault-finding.
- Efficient debug log, который не тормозит event tap hot path.
- Local logs без отправки данных в сеть.
- Wheel mouse detection.
- Step size slider для wheel mouse.
- Step size должен управлять количеством lines per wheel step.
- Step size feature можно отключить, вернув system default behavior.
- Step size UI показывается только когда обнаружен non-continuous/wheel scroll.
- Start at login.
- Show in menu bar toggle.
- Hide menu bar icon без остановки app.
- Понятный способ вернуть icon, если он скрыт.
- Safe uninstall story: quit app, remove app, optional remove preferences.
- Preferences storage в OS-native location.
- Defaults для fresh install.
- Atomic save настроек.
- Восстановление предыдущего рабочего config при invalid config.
- Wake from sleep recovery или relaunch strategy.
- Remote desktop/raw input mode equivalent.
- Документированное ограничение: swipe gestures не reversed.
- Документированное ограничение: custom gesture scrolling surfaces могут не поддерживаться.
- Документированное ограничение: Calendar/iPhone Mirroring-like UI может обходить scroll events.
- Документированное ограничение: trackpad может определяться как mouse при конфликтующих accessibility gestures.
- Документированное ограничение: старые или сторонние trackpads могут не дать нужные signals.

## P1: важные фичи после MVP

- Update checking.
- Manual `Check for updates`.
- Automatic update checks.
- Include beta versions setting.
- Native dark mode.
- Native light mode.
- Retina-quality status icon.
- Modern app icon.
- Re-launching app while already running opens preferences.
- AppleScript или CLI automation equivalent для enable/disable.
- CLI command `enable`.
- CLI command `disable`.
- CLI command `toggle`.
- CLI command `doctor`.
- CLI command `reset-config`.
- Test window или test scroll area.
- Device activity preview: последнее устройство, последнее событие, примененное правило.
- Export diagnostics.
- Copy diagnostics summary.
- Version/build info in diagnostics.
- Homebrew cask/package distribution plan.
- Self-contained install where possible.
- Code signing/notarization plan для macOS.
- Universal macOS build where applicable: Intel and Apple silicon.
- Release notes.
- Changelog.
- Privacy section.
- Security section для input hooks.
- Recovery instructions for broken permissions.
- UX state for missing Accessibility permission.
- UX state for missing Input Monitoring permission.
- UX state for app paused.
- UX state for hook failed.
- UX state for no devices detected.
- UX state for wheel detected.
- UX state for hidden icon.
- Microcopy explaining natural vs classic scrolling.
- Recommended settings shown in docs.

## P2: совместимость, качество и polishing

- Localization framework.
- Russian localization.
- English localization.
- Community translation workflow.
- Stable localized string keys.
- Accessibility labels for controls.
- Keyboard navigation in settings.
- Screen reader-friendly permission states.
- No color-only state communication.
- Compact system utility layout.
- No landing-page UI.
- No decorative gradients/orbs.
- Small binary size budget.
- Dependency audit.
- License audit.
- Crash-safe shutdown.
- Panic hook that restores pass-through behavior.
- Performance budget for event hot path.
- Benchmark for scroll transform.
- Stress test for scroll bursts.
- Regression tests for horizontal scroll.
- Regression tests for vertical scroll.
- Regression tests for step size.
- Regression tests for disabled app.
- Regression tests for disabled device rule.
- Regression tests for unknown device fallback.
- Contract tests for platform traits.
- Manual QA script for first launch.
- Manual QA script for permissions denied.
- Manual QA script for mouse only.
- Manual QA script for trackpad only.
- Manual QA script for Magic Mouse.
- Manual QA script for remote desktop mode.
- Manual QA script for wake from sleep.
- Manual QA script for hidden menu icon recovery.
- Compatibility notes for Magic Trackpad.
- Compatibility notes for MacBook trackpad.
- Compatibility notes for Magic Mouse.
- Compatibility notes for Mighty Mouse-style devices.
- Compatibility notes for Wacom mouse behavior.
- Compatibility notes for wheel mouse acceleration.
- Compatibility notes for high-resolution wheels.
- Compatibility notes for horizontal wheels.
- Compatibility notes for remote desktop and virtual machines.

## Acceptance matrix

| Area | Target behavior | Status | Gap / next action |
| --- | --- | --- | --- |
| Core | Reverse scroll direction | Done | Physical wheel and continuous precision paths are tested separately. |
| Devices | Mouse, trackpad and Magic Mouse independent settings | Implemented | Public two-finger timing classifier and separate live toggles are wired; physical hardware and rapid-alternation QA remains open. |
| Axes | Vertical and horizontal toggles | Done | Both policy and CGEvent field writes have regression tests. |
| Wheel | Step size control | Partial | Implemented; detection-driven conditional visibility is still open. |
| UI | Menu bar app with preferences | Done | Handoff 1b/1e implemented. |
| Pause | Temporary pause without changing settings | Done | 15-minute auto-resume and Resume Now exist in settings and tray. |
| Permissions | Accessibility and Input Monitoring flow | Done | Permission-first tab and separate pane actions exist; live human QA remains. |
| Debug | Option-click debug console | Done | Search/filter/export/clear and bounded local ring buffer exist. |
| Startup | Start at login | Done | GUI uses SMAppService; lean CLI keeps LaunchAgent support. |
| Status icon | Retina menu status and app identity | Done | Template glyph, colored state dot, SVG-to-ICNS app icon pipeline. |
| Hide icon | Show/hide menu bar icon | Open | Requires a recovery/focus command before exposing the toggle. |
| Automation | Scriptable enable/disable | Done | CLI `enable`, `disable`, `toggle`, `doctor`; AppleScript property is not implemented. |
| Updates | Explicit update strategy | Open | Choose Sparkle/manual/no-updater before activating stored flags. |
| Localization | Russian and English-ready strings | Open | User-facing copy still lives inline. |
| Distribution | Signed/notarized release | Open | Local bundle is ad-hoc signed; Developer ID/notarization remains external release work. |
| Install | Stable install, update and uninstall | Implemented | Atomic temp-destination smoke passes; real `/Applications` and login-item cleanup QA remains. |
| Limits | Gestures not reversed | Documented | Keep compatibility notes and hardware QA current. |

## First implementation slices

1. Core config with global enabled, reverse mouse, reverse trackpad, reverse vertical, reverse horizontal.
2. Pure scroll transformer with tests.
3. Device classifier interface with mock implementation.
4. Permission checker interface with mock implementation.
5. Step size domain model and tests.
6. Runtime state: active, paused, needs permission, degraded.
7. CLI `doctor` showing config, permissions and last known device state.
8. macOS feasibility spike for event tap and gesture classification.
9. Menu bar/settings UI shell.
10. Debug console backed by efficient ring buffer.
