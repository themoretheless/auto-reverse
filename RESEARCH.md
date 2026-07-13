# Research: scrolling, device policy, and macOS utility design

Дата прохода: 2026-07-14.

Этот документ фиксирует дополнительный исследовательский проход после анализа
Scroll Reverser: 10 других популярных open-source macOS-утилит, научные работы
по scrolling/latency/filtering и официальные platform materials. Это источник
гипотез, а не обещание немедленно перенести все найденные функции.

## Метод

- Изучались исходники, тесты и design notes, а не только README.
- GitHub stars приведены как снимок на дату прохода и служат только сигналом
  распространенности. Они не доказывают качество архитектуры или алгоритма.
- Идеи проверялись против текущих инвариантов Auto Reverse и лицензий.
- Подходы с private MultitouchSupport API, захватом HID-устройства или записью
  дополнительных scroll-полей не предлагаются к переносу.
- Новые рекомендации имеют IDs `R01-R60` в `recommendation.md`.

## 10 дополнительных репозиториев

Scroll Reverser в эту десятку не входит: он уже разобран отдельно в
`scroll-reverser-parity.md`.

### 1. [Stats](https://github.com/exelban/stats) - 40,422 stars

Изучено: разделение readers, settings и menu modules, lifecycle обновлений,
pause и threshold-based notifications.

Что применимо:

- запускать дорогой reader только когда его данные реально нужны;
- временная пауза не должна переписывать постоянный `enabled`;
- добавлять hysteresis к health warnings, чтобы единичный сбой не мигал в UI;
- держать settings import/export/reset как отдельные операции.

Граница: Auto Reverse не нужен универсальный plugin framework. Несколько
маленьких traits оправданы только после появления второго реального алгоритма.

### 2. [MonitorControl](https://github.com/MonitorControl/MonitorControl) - 33,685 stars

Изучено: стабильная идентичность внешних устройств, duplicate labels,
контекстный target selection и progressive disclosure настроек.

Что применимо:

- показывать friendly alias, но хранить стабильный технический selector;
- различать одинаковые устройства suffix-меткой, не подменяя identity именем;
- убирать редкие параметры в Advanced, сохраняя основной экран коротким;
- явно показывать, какое устройство сейчас является target.

Граница: выбор дисплея по курсору нельзя механически переносить на input
device. Для scroll-события нужен отдельный доказуемый источник attribution.

### 3. [Rectangle](https://github.com/rxhanson/Rectangle) - 29,476 stars

Изучено: Accessibility lifecycle, login item, versioned import/export,
ограничения импортируемого config и app-specific exclusions.

Что применимо:

- импорт сначала валидировать и показывать как diff, затем подтверждать;
- ограничивать размер файла и отказываться от опасных symlink/permission cases;
- сохранять версию schema в экспортируемом config;
- коммитить slider value в постоянный config после завершения взаимодействия,
  а live preview держать отдельно.

Граница: URL automation и большой набор shortcuts для Auto Reverse пока не
окупают дополнительную attack surface.

### 4. [Karabiner-Elements](https://github.com/pqrs-org/Karabiner-Elements) - 22,468 stars

Изучено: device identifiers, per-device modifications, conditions, profiles,
deduplicated inventory и forward-compatible configuration.

Что применимо:

- явный порядок selector specificity;
- address fallback только при отсутствии нормальных VID/PID/serial данных;
- исключение virtual devices;
- три состояния profile field: inherit, on, off;
- сохранение незнакомых config fields при безопасной миграции.

Граница: полноценный condition language слишком велик для v1 Auto Reverse.
Сначала достаточно exact device selector и, отдельно, bundle-id исключений.

### 5. [Mos](https://github.com/Caldis/Mos) - 20,842 stars

Изучено: `ScrollCore`, display-link scheduling, synthetic-event tagging,
per-axis behavior, app exceptions, gesture lifecycle и stale-frame protection.

Что применимо:

- помечать собственные synthetic events и гарантированно не обрабатывать их
  повторно;
- разделять state по осям;
- использовать generation token и TTL, чтобы старый scheduled frame не попал
  в новую scroll session;
- останавливать scheduler, когда momentum отсутствует;
- фиксировать target process на всю одну scroll session.

Не переносить: Mos изменяет point delta/phase-related semantics. Auto Reverse
должен продолжать писать только `DeltaAxis1/2`; fixed-point и pixel deltas
macOS выводит сама.

### 6. [AltTab](https://github.com/lwouis/alt-tab-macos) - 16,032 stars

Изучено: event-tap recovery, permission polling, feature-dependent permission
callouts, preferences search и login migration.

Что применимо:

- опрашивать permission чаще только пока permission UI видим, затем переходить
  на редкий backstop;
- показывать callout лишь когда включенная функция действительно заблокирована;
- добавить fuzzy search после роста количества настроек;
- различать disabled tap и отсутствующие разрешения в recovery telemetry.

Граница: Screen Recording permission и window enumeration не имеют отношения
к Auto Reverse и не должны появляться в onboarding.

### 7. [Hammerspoon](https://github.com/Hammerspoon/hammerspoon) - 15,725 stars

Изучено: lifecycle event taps, cleanup, callback error isolation, secure-input
diagnostics и re-enable paths.

Что применимо:

- callback не должен переживать уничтоженное состояние;
- ошибка пользовательского/внешнего callback не должна рушить event loop;
- cleanup обязан быть idempotent;
- диагностика должна отличать disabled-by-timeout от normal shutdown.

Не переносить: части touch support используют private API. Для Auto Reverse
остается только public AppKit listen-only gesture bridge с raw event type 29.

### 8. [Mac Mouse Fix](https://github.com/noah-nuebling/mac-mouse-fix) - 10,425 stars

Изучено: `ScrollNotes.md`, `ScrollConfigTesting.md`, `ScrollAnalyzer`,
`ScrollControl`, device manager и compatibility notes.

Что применимо:

- подбирать кривую на сохраненных traces и реальных задачах, не на ощущении от
  одного приложения;
- противоположный tick должен сразу гасить старое momentum;
- stop threshold нужен против остаточного pixel creep;
- compatibility matrix должна включать Safari zoom, Launchpad,
  iOS/iPad-style apps, Universal Control и iPhone Mirroring.

Граница: экспериментальный polling-rate код помечен самим проектом как
проблемный. Его нельзя копировать как готовое решение.

### 9. [LinearMouse](https://github.com/linearmouse/linearmouse) - 6,484 stars

Изучено: device matching, configuration schemes, per-app conditions,
scroll presets, input-rate estimation, momentum state и event-tap watchdog.

Что применимо:

- чистый dynamics engine должен принимать normalized sample и time delta;
- `dt` должен иметь разумные bounds после sleep/stall;
- per-axis residual и momentum не должны смешиваться;
- re-engagement и opposite-direction input требуют отдельных переходов state;
- profile resolver может объединять device, app и default layers.

Граница: богатый scheme engine и все параметры кривой нельзя сразу выставлять
в UI. Сначала нужны 3-4 понятных presets и Advanced только для диагностики.

### 10. [UnnaturalScrollWheels](https://github.com/ther0n/UnnaturalScrollWheels) - 4,137 stars

Изучено: компактный CGEventTap, continuous/discrete heuristic, tap re-enable,
sleep/wake recovery и preference keys.

Что применимо:

- после wake делать bounded immediate retry и один delayed retry;
- при disabled tap сначала проверять реальное состояние, затем пересоздавать;
- держать простую fail-open политику: при ошибке пропускать исходный event;
- иметь migration fixture на каждый исторический config key.

Отдельный урок: опечатка между write/read preference key легко переживает
code review. Typed schema и migration tests важнее ручной внимательности.

## Научные работы

### Scrolling speed and accuracy

1. Hinckley et al., [Quantitative Analysis of Scrolling Techniques](https://www.microsoft.com/en-us/research/publication/quantitative-analysis-of-scrolling-techniques/), CHI 2002. Wheel оказался сильным на коротких дистанциях, а acceleration заметно помогает на длинных. Значит, одна универсальная gain curve не должна считаться доказанно лучшей.
2. Chen et al., [ScrollTest: Evaluating Scrolling Speed and Accuracy](https://arxiv.org/abs/2210.00735), 2022. Полезные продуктовые метрики: movement time, switchbacks и maximum overshoot; тесты разделяют известное и неизвестное положение цели, расстояние и размер viewport.
3. Quinn et al., [Exposing and Understanding Scrolling Transfer Functions](https://direction.bordeaux.inria.fr/~roussel/publications/2012-UIST-scrolling-tf.pdf), UIST 2012. Transfer function зависит от velocity, direction, duration и clutching; разные detent wheels могут выглядеть одинаково на уровне generic counts. Нужен trace lab, а не hardcoded вывод по одному устройству.

### Filtering and latency

4. Casiez, Roussel, Vogel, [1 Euro Filter](https://gery.casiez.net/1euro/), CHI 2012. Speed-adaptive cutoff дает практический компромисс jitter/lag. Для Auto Reverse это кандидат для noisy rate estimate или classifier signal, но не для изменения исходной scroll distance без отдельного эксперимента.
5. MacKenzie and Ware, [Lag as a Determinant of Human Performance in Interactive Systems](https://www.yorku.ca/mack/CHI93b.html), INTERCHI 1993. Задержка резко ухудшала pointing performance. Точные числа нельзя напрямую объявлять scroll threshold, но работа подтверждает необходимость измеримого callback/scheduler latency budget.
6. Jota et al., [How Fast Is Fast Enough?](https://www.tactuallabs.com/papers/howFastIsFastEnoughCHI13.pdf), CHI 2013. Эффект latency нелинеен и зависит от задачи. Для Auto Reverse вывод простой: оптимизация ниже измеримого порога менее ценна, чем устранение long-tail stalls.

## Технические и platform materials

- [libinput scrolling](https://wayland.freedesktop.org/libinput/doc/latest/scrolling): wheel, finger и continuous scroll имеют разные semantics; kinetic scrolling уместен не для каждого source; axis lock применяется после порога.
- [libinput wheel API](https://wayland.freedesktop.org/libinput/doc/1.31.0/wheel-api.html): high-resolution stream и legacy stream нельзя бездумно суммировать; firmware может сообщать неточные resolution данные.
- [Apple Quartz Event Services](https://developer.apple.com/documentation/coregraphics/quartz-event-services): публичная основа low-level event tap.
- [Apple kCGEventSourceUserData](https://developer.apple.com/documentation/coregraphics/cgeventfield/eventsourceuserdata): публичное 64-bit user-supplied поле event source; Auto Reverse использует его только для self-synthetic marker.
- [Apple CGGetEventTapList](https://developer.apple.com/documentation/coregraphics/cggeteventtaplist(_:_:_:)): публично доступны enabled state и min/average/max latency tap. Чтение min/max меняет последующую выборку, поэтому UI должен маркировать метрику как interval snapshot.
- [Apple IOHIDManager](https://developer.apple.com/documentation/iokit/iohidmanager_h): публичные matching, add/remove и input callbacks остаются источником inventory.
- [Apple Energy Efficiency Guide](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/Timers.html): notifications предпочтительнее polling, неиспользуемые timers нужно выключать, а background timers должны иметь tolerance.
- [Apple CoreHID HIDDeviceManager](https://developer.apple.com/documentation/corehid/hiddevicemanager): возможный future adapter для новых macOS, но не основание повышать minimum deployment target без отдельного availability spike.

## Синтез

Главный вывод: "плавный скролл" нельзя добавлять одним коэффициентом. Сначала
нужна измерительная база, затем чистая state machine только для discrete wheel,
и только потом UX presets. Трекпад и Magic Mouse уже несут continuous/momentum
semantics macOS; повторное сглаживание ухудшит latency и расстояние.

Предлагаемый pipeline:

```text
CGEvent
  -> one immutable HID/gesture snapshot
  -> device classification
  -> profile resolution
  -> pure reversal policy
  -> optional discrete-wheel dynamics
  -> write DeltaAxis1/2 only
  -> bounded local diagnostics
```

Предлагаемые маленькие boundaries, создаваемые только по мере реализации:

```text
src/scroll_trace.rs                       pure trace schema and replay
src/statistics.rs                         shared nearest-rank distributions
src/event_rate.rs                         observed delivery-rate histogram
src/scroll_benchmark.rs                   pure target-acquisition state machine
src/scroll_dynamics.rs                    pure scalar-axis dynamics state machine
src/scroll_scheduler.rs                   pure wake/fail-open orchestration
src/scroll_scheduler/schedule.rs          generation, wake id and TTL contract
src/config/profiles.rs                    inheritance and selector resolution
src/platform/macos/tap_metrics.rs         CGGetEventTapList diagnostics
src/platform/macos/scroll_scheduler.rs    tagged, bounded output scheduling
```

Pure `scroll_scheduler.rs` не принимает решений о кривой: он выдает wake token
и tagged sample, отбрасывает stale generation/wake/TTL и latch-ит fail-open при
любой ошибке. Будущий platform adapter только ставит public synthetic marker и
пишет `DeltaAxis1/2`. Это сохраняет SRP и не затягивает CoreGraphics в domain
layer.

## Три исследовательские итерации

### Итерация A: измерение без изменения поведения

1. [Done R01-R03] Добавить privacy-bounded trace schema и pure replay.
2. [Implemented R10] Снимать callback latency только вручную через public
   `CGGetEventTapList`; live UI QA остается.
3. [Implemented R04-R09] Transfer lab, constant baseline, Known/Unknown
   ScrollTest-style harness, deterministic case matrices и observed event-rate
   distributions готовы; physical-device/visual QA остается.
4. [Implemented R11-R12] Callback/scheduler budgets, repeated-stall policy и
   шесть stable physical test strata зафиксированы; реальные прогоны всех
   устройств и приложений остаются manual QA.

Критерий выхода: текущий raw/reverse behavior не изменился; trace не хранит
текст, app title или произвольные HID payloads; benchmark воспроизводим.

### Итерация B: opt-in discrete-wheel dynamics

1. [Done R13-R15 boundary] Измеримый contract, четыре presets и pure
   scalar-axis engine готовы без live integration.
2. [Done R16-R20] Continuous bypass, transactional independent axis states,
   1-50 ms `dt`, median 3-of-8 recent-rate estimate и signed-distance ledger
   готовы в pure model.
3. [Done R21-R25] Direction reset, opposite-input cancellation, 150 ms gap
   sessions, 0.25 pt stop threshold и explicit click/action policy готовы с
   signed cancellation accounting.
4. [Done R26-R30 pure boundary] Tagged wake/sample contract, generation+TTL,
   idle lifecycle, latched fail-open и benchmark-only height hypothesis готовы;
   platform timer и runtime opt-in отсутствуют.
5. [Done pure contract] При любой dynamics/scheduler ошибке текущий event
   возвращается точно, pending wake очищается, дальнейшие события bypass-ятся
   до explicit reset.

Критерий выхода: continuous Trackpad/Magic Mouse path не изменен; output
меняет только `DeltaAxis1/2`; нет self-feedback, stale frames и pixel creep.
Height hypothesis использует controlled viewport как proxy с baseline default;
это instrumentation для evidence, а не новая runtime transfer function.

### Итерация C: profiles and product UX

1. Расширить `DeviceRule` optional полями step/preset через migration.
2. Добавить inherit/on/off resolution и объяснение active rule.
3. Добавить device test row, settings search и versioned import dry-run.
4. Прогнать compatibility matrix и оставить kill switch для dynamics.

Критерий выхода: основной экран остается компактным; advanced controls не
показывают внутренние коэффициенты большинству пользователей; rollback не
требует ручного редактирования TOML.

## Что сознательно отвергнуто

- private MultitouchSupport и undocumented touch APIs;
- вывод "нет two-finger observation = Magic Mouse";
- HID seize или собственный kernel/DriverKit driver ради v1;
- запись fixed-point, pixel delta, point delta или phase fields;
- повторное smoothing continuous trackpad/Magic Mouse events;
- бесконечный polling permission/device state;
- auto-tuning кривой без явного opt-in и обратимого preview;
- копирование чужого алгоритма или кода без license/provenance review;
- telemetry/network upload traces по умолчанию;
- использование stars как доказательства корректности.

## Приоритет

Пакеты 1-6 (`R01-R30`) реализованы без изменения live scroll policy:
trace/replay/lab, ScrollTest-style benchmark, observed rates, repeated latency
assessment, physical test strata, measurable dynamics contract и pure
two-axis dynamics model с continuous bypass, bounded time/rate и conservation
ledger, session reset, cancellation/stop policy и pure scheduler safety contract.
Следующий пакет - `R31-R35`: profile fields, precedence и source identity.
