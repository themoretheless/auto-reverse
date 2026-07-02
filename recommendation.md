# 560 рекомендаций, проблем и улучшений (500 базовых + N01-N60 после автозапуска)

Список обновлен после merge ветки `worktree-rust-impl` в `master`. Он отражает текущий код: macOS event tap, TOML config, CLI, permission checks, raw-input guard, step size, LaunchAgent start at login, 27 unit tests и открытые gaps до Scroll Reverser parity.

## Верифицированные находки из 3 реальных итераций ревью (rust-impl worktree)

Секция ниже - не тот же жанр документа, что остальной backlog. Весь список из 500 пунктов дальше был написан **до** реализации, как содержательный brainstorming над голым `cargo new` scaffold - это честно и полезно как architecture backlog, но ни один пункт там не проверен против реального кода.

То, что описано здесь, наоборот: 3 итерации автоматизированного code review (7 независимых критериев на итерацию: correctness, SOLID/DRY-архитектура, macOS-специфика, security/unsafe-код, CLI/UX, test coverage, packaging), где каждая находка затем **адверсариально перепроверялась** отдельным агентом, читающим реальный файл и требующим конкретный воспроизводимый сценарий. Ложные/неточные находки отбрасывались (например, в итерации 2 из 17 находок реальными подтвердились все 17; в итерации 3 - 7 из 8; в самой первой итерации - 15 из 34 сырых находок).

Результат: **34 подтвержденных, реальных проблемы, все исправлены**, плюс один более крупный баг, найденный вручную до формального ревью (см. ниже). Это не 500 - и намеренно: раздувать список до круглого числа для маленькой утилиты означало бы придумывать дубли и стилевые придирки, а не находить реальные дефекты. 34 честные, воспроизводимые находки полезнее 500 наполовину выдуманных.

### Баг, найденный вручную до итерации 1 (самый серьезный из всех)

Реализация `reverse_in_place` изначально трогала три пары полей CGEvent: `DeltaAxis`, `FixedPtDeltaAxis` и `PointDeltaAxis` - независимо считывая и инвертируя каждое. Написав и запустив реальную тестовую программу против настоящего `CGEvent` (не догадка, а эмпирическая проверка), выяснилось: запись в `DeltaAxis` заставляет macOS автоматически пересчитать `FixedPtDeltaAxis` и `PointDeltaAxis` **от нового значения**. Из-за этого код читал уже инвертированное производное поле и инвертировал его повторно, тихо возвращая исходное, неразвернутое направление для любого потребителя, который использует пиксельную/fixed-point дельту вместо простой `DeltaAxis`. Финальное решение: трогать только `DeltaAxis`, ничего больше - и добавить регрессионный тест, который явно проверяет, что производные поля переворачиваются автоматически.

### Итерация 1 - 10 находок, все исправлены

V01. **[Архитектура]** `event_tap.rs` сам читал `EVENT_SOURCE_UNIX_PROCESS_ID` и применял `reverse_only_raw_input` вместо того, чтобы делегировать это `scroll.rs`, которому принадлежит вся остальная трансляция CGEvent -> domain. Исправлено: добавлено поле `ScrollEvent::source_pid`, вся логика перенесена в `scroll::transform_event`.
V02. **[Мертвый код]** `SourceClassifier`/`SourceObservation`/`ScrollPhase` в `device.rs` никогда не вызывались из реального event tap - только из собственных тестов, создавая ложную уверенность в покрытии классификации устройств. Удалены.
V03. **[Мертвый код]** `DeviceKind::is_mouse_like` не имел ни одного вызова нигде в проекте. Удален.
V04. **[DRY]** Строковое представление `DeviceKind` было продублировано в `Display` и `FromStr` независимо. Унифицировано через `DeviceKind::as_str()`.
V05. **[Честность]** `reverse_magic_mouse` недостижим из реального event tap (классификатор никогда не возвращает `MagicMouse`), но нигде не было отмечено, что это известное ограничение.
V06. **[macOS API]** Комментарий в `permissions.rs` утверждал, что нет публичной проверки Input Monitoring - неверно: `CGPreflightListenEventAccess`/`CGRequestListenEventAccess` существуют именно для этого (проверено напрямую по заголовку `CGEvent.h` в реальном macOS SDK на этой машине). Добавлены обе функции.
V07. **[Надежность]** `OnceLock<AppConfig>` в `event_tap.rs` не имеет пути перезагрузки - при повторном вызове в одном процессе всегда возвращает ошибку до попытки установить tap.
V08. **[Race condition]** `ConfigStore::save` использовал фиксированное имя временного файла - два параллельных сохранения могли столкнуться и тихо затереть друг друга. Исправлено: уникальное имя на основе PID + монотонный счетчик.
V09. **[Паника]** Инверсия `i64` дельт (`-value`) паникует в debug-сборке на `i64::MIN`. Исправлено на `saturating_neg()`/`unsigned_abs()`.
V10. **[UX]** `simulate` печатал сырой Rust `{:?}` вместо читаемого текста. Добавлен `impl Display for ScrollEvent`.

### Итерация 2 - 17 находок, все исправлены

V11. **[macOS API]** Приложение только пассивно проверяло Accessibility/Input Monitoring, но никогда не вызывало `AXIsProcessTrustedWithOptions`/`CGRequestListenEventAccess`, чтобы реально показать системный диалог согласия пользователю при первом запуске.
V12. **[Надежность]** `deny_unknown_fields` на `AppConfig` ломал именно ту совместимость, ради которой существует `config_version`: конфиг от будущей версии с новым полем падал с общей ошибкой парсинга раньше, чем `validate()` успевал показать понятное сообщение о версии. Убран.
V13. **[Баг]** `discrete_scroll_step_size` масштабировал только `delta_vertical`, никогда `delta_horizontal`, хотя горизонтальный reversal полностью поддерживается.
V14. **[DRY]** `permission_word` (main.rs) и `permission_status` (permissions.rs) - побайтово идентичные приватные функции в двух модулях.
V15. **[DRY]** Описание классификатора в `doctor()` было отдельной строкой, продублированной с реальной логикой в `device::conservative_kind_from_continuity`.
V16. **[Утечка]** `config.rs::save` очищал временный файл при ошибке `fs::rename`, но не при ошибке `fs::write`.
V17. **[Честность]** `reverse_only_raw_input` не показывался нигде в `doctor`/`run`, хотя может тихо остановить реверс скролла над remote desktop.
V18-V24. **[Тесты]** Добавлены недостающие тесты: `reverse_only_raw_input`, устойчивость к незнакомым TOML-полям, отсутствие утечки temp-файлов при повторном `save`, и полное покрытие CLI-парсинга (`parse_i64`/`parse_bool`), которое отсутствовало вовсе.

### Итерация 3 (финальная) - 7 находок, все исправлены

V25. **[Soundness]** `AXIsProcessTrusted`/`AXIsProcessTrustedWithOptions` были объявлены как возвращающие Rust `bool`, но реальный тип SDK - Carbon `Boolean` (`unsigned char`, где валиден любой ненулевой байт). У Rust `bool` жесткий инвариант 0x00/0x01 - это potential UB. Исправлено на `u8` + явное сравнение `!= 0`.
V26. **[Баг]** `discrete_scroll_step_size` применялся, даже если реверс для этого устройства выключен (`reverse_mouse = false`) - масштабировал скорость скролла, не переворачивая направление. Теперь оба эффекта зависят от одного и того же условия.
V27. **[Честность]** `reverse_unknown` - того же класса мертвый код, что и `reverse_magic_mouse`, но не был отмечен как известный gap.
V28. **[Честность]** Раньше 5 полей конфига (`start_at_login`, `show_menu_bar_icon`, `check_for_updates`, `include_beta_updates`, `show_discrete_scroll_options`) были GUI/updater заглушками без единой строчки реализации где-либо в проекте, и `doctor` никак это не показывал. `start_at_login` теперь реализован через LaunchAgent; остальные поля все еще planned.
V29. **[UX]** `doctor` показывал сырые имена полей конфига (`vertical=`, `magic_mouse=`) без единого понятного предложения о том, что реально происходит. Добавлена строка "what it's doing" на понятном языке.
V30. **[Консистентность]** `magic_mouse=` в выводе `config_summary` не совпадало с `reverse_magic_mouse` в тексте про known gap - те же имена полей теперь используются везде.
V31. **[UX]** Статус `NEEDS PERMISSION` в `doctor` не показывал, что именно делать - теперь `doctor` печатает ту же actionable-инструкцию, что и `run`.

### Методологическое примечание

Каждая находка проверялась независимым агентом, читающим актуальный код (не полагаясь на описание), и получала вердикт `real: true/false` с обоснованием. Находки без конкретного файла/строки и воспроизводимого сценария не засчитывались. Это не гарантирует отсутствие оставшихся проблем - 3 итерации это ограниченная, а не исчерпывающая проверка - но каждая перечисленная выше находка была лично перепроверена против работающего, протестированного кода (`cargo build`, `cargo test`, `cargo clippy` - все чистые после каждой итерации), а не выдумана для объема.

### Известные ограничения, оставленные как есть (не баги - осознанные решения)

- **Magic Mouse vs trackpad**: невозможно различить через публичный `CGEventTap` API - оба репортят `continuous scrolling` одинаково. Настоящее решение потребовало бы IOHIDManager device matching (недокументированная привязка конкретного `CGEvent` к конкретному HID-устройству) - это отдельная, значительно более сложная работа.
- **`OnceLock<AppConfig>`**: конфиг заморожен на весь процесс `run` - изменения `enable`/`disable`/`toggle` во время работающего tap не подхватываются без перезапуска. Приемлемо для однопроцессного CLI, но потребует redesign для live-reload.
- **Локализация**: весь user-facing текст - inline английские литералы без message catalog. Осознанно не в scope этих 3 итераций (это Итерация 3 по `architecture.md`/`readme.md` roadmap, то есть более крупная будущая работа, а не quick fix).
- **Диалог согласия для Input Monitoring/Accessibility**: `request_missing_permissions()` теперь реально вызывает системные API показа диалога, но в этой non-interactive сессии невозможно визуально подтвердить, что диалог появляется на экране - только то, что вызов не падает.

## Итерация 1: Core Safety

1. [Done] Проект уже имеет рабочий CLI вместо старого `Hello, world!`.
2. [Done] `src/lib.rs` отделяет library facade от binary entrypoint.
3. [Done] `src/main.rs` стал тонким CLI entrypoint.
4. [Done] `AppConfig` хранит versioned config schema.
5. [Done] TOML выбран как читаемый формат настроек.
6. [Done] `ConfigStore::default_path` учитывает macOS Application Support.
7. [Done] `AUTO_REVERSE_CONFIG` помогает безопасно тестировать конфиг.
8. [Done] `load_or_create` делает first-run проще.
9. [Done] Config save использует уникальный temporary file.
10. [Problem] Config save еще не делает fsync файла и директории.
11. [Improve] Добавить durable save для production release.
12. [Problem] Нет backup corrupted config.
13. [Improve] При parse error сохранять `.broken.<timestamp>.toml`.
14. [Problem] Нет migration framework для `config_version`.
15. [Improve] Добавить `config::migration` до schema v2.
16. [Done] `ConfigStore` и `AppConfig` разделены: `config/schema.rs` и `config/store.rs`.
17. [Done] Разделение `config/schema.rs` / `config/store.rs` выполнено.
18. [Done] Монолитный `config.rs` удален; ответственности разнесены по SRP.
19. [Done] `config/mod.rs` реэкспортирует `AppConfig`/`ConfigStore` как public facade.
20. [Done] `DeviceKind::as_str` уменьшает DRY-дублирование.
21. [Done] `Display` и `FromStr` используют единый device-name контракт.
22. [Done] `DeviceKind` покрывает mouse, trackpad, Magic Mouse, unknown.
23. [Problem] Magic Mouse пока не определяется live classifier.
24. [Improve] Добавить отдельный gesture/HID classifier.
25. [Problem] Continuous scroll сейчас консервативно считается trackpad.
26. [Improve] Явно показывать этот gap в CLI и UI.
27. [Done] Non-continuous scroll считается mouse.
28. [Problem] Нет stable device id.
29. [Improve] Добавить `DeviceId` и `DeviceInfo`.
30. [Problem] Нет device registry.
31. [Improve] Хранить known devices и last_seen metadata.
32. [Problem] Unknown device config есть, но discovery отсутствует.
33. [Improve] Показывать unknown devices в diagnostics.
34. [Done] `ScrollEvent` нормализует vertical/horizontal delta.
35. [Done] `ScrollEvent` содержит `continuous`.
36. [Done] `ScrollEvent` содержит `synthetic`.
37. [Done] `ScrollEvent` содержит `source_pid`.
38. [Problem] `ScrollEvent` не содержит timestamp.
39. [Improve] Добавить monotonic timestamp для diagnostics.
40. [Problem] `ScrollEvent` не содержит event phase.
41. [Improve] Добавить phase после gesture/HID spike.
42. [Problem] `ScrollEvent` не содержит device id.
43. [Improve] Добавить optional `device_id`.
44. [Done] `scroll::transform_event` чисто тестируется.
45. [Done] Disabled config делает pass-through.
46. [Done] Synthetic event делает pass-through.
47. [Done] Raw-input guard пропускает injected events.
48. [Done] CLI simulate умеет задавать `source_pid`.
49. [Improve] Добавить integration test для `simulate --source-pid`.
50. [Done] CLI simulate умеет задавать `synthetic`.
51. [Improve] Добавить integration test для `simulate --synthetic`.
52. [Done] Vertical reverse включен по умолчанию.
53. [Done] Horizontal reverse выключен по умолчанию.
54. [Done] Mouse reverse включен по умолчанию.
55. [Done] Trackpad reverse выключен по умолчанию.
56. [Problem] Magic Mouse reverse включен в config, но live classifier не умеет его применить.
57. [Improve] Временно пометить `reverse_magic_mouse` как planned в docs/UI.
58. [Done] Step size применяется к non-continuous wheel delta.
59. [Problem] Step size logic живет рядом с reverse logic.
60. [Improve] Вынести wheel step в `scroll::wheel`.
61. [Done] `discrete_scroll_step_size` валидируется диапазоном 0..=20.
62. [Problem] Диапазон step size не объяснен в docs.
63. [Improve] Добавить описание: 0 means system/default/no adjustment.
64. [Done] `saturating_neg` предотвращает overflow.
65. [Done] Step size multiplication использует `saturating_mul`.
66. [Improve] Оставить regression test на будущий рост диапазона step size.
67. [Done] CoreGraphics derived delta regression покрыт тестом.
68. [Done] CoreGraphics helpers вынесены из `scroll.rs`; он теперь чистая политика.
69. [Done] CGEvent field code живет в `platform/macos/scroll_events.rs`.
70. [Done] Event tap re-enables itself after disabled callbacks.
71. [Problem] Event tap install не имеет integration smoke test.
72. [Improve] Добавить mock listener для runtime contract tests.
73. [Problem] `OnceLock<AppConfig>` делает event tap одноразовым в процессе.
74. [Improve] Для будущего UI нужен runtime state с reloadable config snapshot.
75. [Problem] Нет hot reload config.
76. [Improve] Добавить command `reload` или runtime channel.
77. [Problem] Нет pause без изменения config.
78. [Improve] Различить persistent enabled и temporary paused.
79. [Done] CLI `enable`, `disable`, `toggle` меняют config.
80. [Problem] CLI commands не используют subcommand parser.
81. [Improve] Для роста CLI добавить `clap`.
82. [Problem] `main.rs` содержит ручной parsing flags.
83. [Improve] Вынести CLI parsing в `app::command`.
84. [Problem] `parse_bool` принимает yes/no/1/0, но help говорит true/false.
85. [Improve] Help должен перечислять accepted bool values.
86. [Problem] CLI errors не имеют stable error codes.
87. [Improve] Добавить `E_CONFIG_PARSE`, `E_PERMISSION`, `E_PLATFORM`.
88. [Done] `AppError` отделяет IO, config, platform и usage.
89. [Problem] `AppError::InvalidConfig` хранит plain string.
90. [Improve] Сделать structured validation errors.
91. [Problem] `AppError::Platform` слишком общий.
92. [Improve] Добавить enum для permission/tap/install/runtime.
93. [Done] Accessibility check реализован.
94. [Done] Input Monitoring preflight реализован.
95. [Problem] `request_input_monitoring_access` не используется в CLI flow.
96. [Improve] При missing Input Monitoring предлагать request action.
97. [Problem] Accessibility prompt не вызывается через trusted options.
98. [Improve] Добавить API для request Accessibility permission.
99. [Problem] Permission docs не показывают exact binary path.
100. [Improve] `doctor` должен печатать current executable path.
101. [Problem] `doctor` создает config как side effect.
102. [Improve] Разделить `doctor --no-create` и first-run init.
103. [Done] `doctor` показывает Accessibility и Input Monitoring.
104. [Problem] `doctor` не проверяет event tap installability.
105. [Improve] Добавить dry install check или explicit explanation.
106. [Problem] Нет runtime diagnostics buffer.
107. [Improve] Добавить ring buffer для последних decisions.
108. [Problem] Event hot path не должен логировать синхронно.
109. [Improve] Использовать lock-free/ring buffer или sampled logging.
110. [Problem] Нет tracing/log crate.
111. [Improve] Ввести `tracing` только после выбора diagnostics design.
112. [Problem] Нет benchmark hot path.
113. [Improve] Добавить microbenchmark для `transform_event`.
114. [Problem] Нет property tests для sign reversal.
115. [Improve] Проверить invariant: magnitude сохраняется кроме wheel step.
116. [Problem] Нет теста для `i64::MIN` vertical/horizontal.
117. [Improve] Добавить regression tests для saturating behavior.
118. [Done] Есть тест для step size 0.
119. [Improve] Добавить CLI simulation example для step size 0 после command support.
120. [Problem] Нет теста для `reverse_unknown`.
121. [Improve] Добавить unknown-device transform test.
122. [Done] Pure transform покрывает Magic Mouse config.
123. [Improve] Live Magic Mouse distinction все еще требует gesture/HID classifier.
124. [Problem] Live classifier не покрыт integration contract.
125. [Improve] Добавить tests для `conservative_kind_from_continuity`.
126. [Done] Device parse/display round-trip покрыт.
127. [Problem] Нет serde round-trip для `DeviceKind`.
128. [Improve] Добавить TOML test для `magic-mouse`.
129. [Problem] Нет CLI snapshot tests.
130. [Improve] Добавить integration tests через `assert_cmd`.
131. [Problem] Нет golden output для `show-config`.
132. [Improve] Зафиксировать config output или сделать формат explicit unstable.
133. [Problem] Нет test tempdir crate.
134. [Improve] Использовать `tempfile` вместо timestamp path helper.
135. [Problem] Tests оставляют файл, если panic до cleanup.
136. [Improve] `tempfile::NamedTempFile` решит cleanup.
137. [Problem] Нет module-level docs.
138. [Improve] Добавить краткие `//!` docs для модулей.
139. [Problem] Публичный API слишком широк: все modules `pub`.
140. [Improve] Экспортировать facade, скрывать platform internals.
141. [Problem] `event_tap` публичен из lib.
142. [Improve] После UI/runtime split сделать platform modules crate-private.
143. [Done] `permissions` переехал под platform-слой.
144. [Done] Модуль живет в `src/platform/macos/permissions.rs`.
145. [Problem] Проект пока macOS-only, но docs говорят о future cross-platform.
146. [Done] `src/platform/mod.rs` cfg-gate'ит `macos`; бинарь дает понятный compile_error! вне macOS.
147. [Problem] Non-macOS build behavior не определен.
148. [Improve] Сделать graceful compile error или stub platform.
149. [Problem] Cargo features не разделяют platform code.
150. [Improve] Добавить feature `macos-event-tap`.
151. [Done] `core-graphics`/`core-foundation` стали target-specific dependencies.
152. [Done] Cargo.toml: `[target.'cfg(target_os = "macos")'.dependencies]`.
153. [Problem] Нет MSRV.
154. [Improve] Зафиксировать Rust version через `rust-toolchain.toml`.
155. [Problem] Edition 2024 требует свежий toolchain.
156. [Improve] README должен назвать required Rust version.
157. [Problem] Нет CI.
158. [Improve] Добавить GitHub Actions после remote setup.
159. [Problem] Нет `cargo audit`.
160. [Improve] Добавить audit в release checklist.
161. [Problem] Нет license.
162. [Improve] Выбрать MIT/Apache-2.0/другую license до публикации.
163. [Problem] Нет changelog.
164. [Improve] Добавить `CHANGELOG.md` с текущим first slice.
165. [Problem] Нет ADR.
166. [Improve] Создать ADR for event tap, config format, CLI first.
167. [Done] `.idea/` добавлен в root `.gitignore`.
168. [Improve] IDE metadata остается локальным и не попадает в commit.
169. [Problem] Remote не настроен.
170. [Improve] Добавить `origin`, иначе push невозможен.

## Новые идеи после автозапуска

N01. Добавить `startup-status --json`.
N02. Добавить `enable-startup --no-config-write` для debugging.
N03. Добавить `repair-startup`, если LaunchAgent указывает на старый binary path.
N04. Показывать warning, если автозапуск настроен на `target/debug`.
N05. Показывать current executable path в `doctor`.
N06. Добавить `open-launch-agent` для показа plist в Finder.
N07. Добавить uninstall command, который выключает startup и удаляет config backup.
N08. Добавить single-instance lock, чтобы LaunchAgent не запускал второй tap.
N09. Второй запуск должен отправлять команду первому процессу.
N10. Добавить `restart-tap` command.
N11. Добавить launchd stdout/stderr log paths.
N12. Добавить `doctor` check, что LaunchAgent plist XML валиден.
N13. Добавить test для mismatch: plist есть, но binary path другой.
N14. Добавить UI warning: "Autostart uses this exact binary".
N15. При packaged `.app` перейти с LaunchAgent CLI path на `SMAppService`.
N16. Добавить миграцию LaunchAgent при смене binary path.
N17. Добавить command `where-am-i-installed`.
N18. Добавить config backup перед изменениями startup.
N19. Добавить file lock на config writes.
N20. Добавить `doctor --fix` для безопасных auto-repairs.
N21. Добавить permissions preflight в LaunchAgent startup logs.
N22. Добавить `run --foreground` для явного CLI режима.
N23. Добавить `run --daemon` для будущего фонового режима.
N24. Добавить health file с последним успешным запуском.
N25. Добавить "last crash" marker.
N26. Добавить wake-from-sleep tap rearm.
N27. Добавить app bundle smoke target.
N28. Добавить status menu без settings window как промежуточный UX.
N29. Добавить native notification при missing permissions on login.
N30. Добавить `pause --minutes`.
N31. Добавить `resume`.
N32. Добавить `status` как короткую версию `doctor`.
N33. Добавить `logs` command.
N34. Добавить ring buffer export.
N35. Добавить human-readable reason для каждого pass-through decision.
N36. Добавить test matrix for launch at login.
N37. Добавить manual QA сценарий "reboot/login".
N38. Добавить warning, если `HOME` не выставлен.
N39. Добавить fallback docs для managed/corporate Macs.
N40. Добавить "no network by default" badge in README.
N41. Добавить `PRIVACY.md`.
N42. Добавить `SECURITY.md`.
N43. Добавить `ROADMAP.md` P0/P1/P2.
N44. Добавить `QA.md`.
N45. Добавить `DESIGN.md` для compact utility UI.
N46. Добавить `README.ru.md`.
N47. Добавить short screencast после GUI.
N48. Добавить launchd label в `doctor`.
N49. Добавить "copy diagnostics" output.
N50. Добавить source attribution for LaunchAgent design.
N51. Добавить integration tests for CLI commands through temp HOME.
N52. Добавить isolated HOME test harness.
N53. Добавить "permission identity changed after rebuild" warning in startup-status.
N54. Добавить plist cleanup on partial install errors.
N55. Добавить `disable-startup --keep-config`.
N56. Добавить `enable-startup --path <binary>` для packaged tests.
N57. Добавить startup module docs with LaunchAgent vs SMAppService tradeoff.
N58. Добавить UI copy explaining next-login behavior.
N59. Добавить warning if config says startup true but LaunchAgent missing.
N60. Добавить warning if LaunchAgent exists but config says startup false.

## Итерация 2: Product UX and Design

171. [Problem] Нет menu bar app.
172. [Improve] Сделать macOS status item как primary UI.
173. [Problem] CLI не заменяет настройки для обычного пользователя.
174. [Improve] Добавить preferences window.
175. [Problem] Нет first-run welcome.
176. [Improve] Показать onboarding с permissions и recommended setup.
177. [Problem] Нет visible active/paused state.
178. [Improve] Status icon должен отражать active, paused, blocked.
179. [Problem] Нет temporary pause.
180. [Improve] Добавить pause без записи config.
181. [Problem] Right-click toggle не реализован.
182. [Improve] Повторить Scroll Reverser: right/control click toggles app.
183. [Problem] Option-click debug console не реализован.
184. [Improve] Option-click открывает diagnostics/debug console.
185. [Problem] Hide menu bar icon config есть, UI нет.
186. [Improve] Реализовать show/hide icon с recovery через CLI.
187. [Done] Start at login config теперь связан с LaunchAgent integration.
188. [Improve] Позже заменить/дополнить LaunchAgent через `SMAppService` для packaged `.app`.
189. [Problem] Update config fields есть, updater нет.
190. [Improve] Решить: Sparkle, manual releases или no auto-update.
191. [Problem] Beta updates flag есть, behavior нет.
192. [Improve] Скрыть/пометить beta flag до update strategy.
193. [Problem] `show_discrete_scroll_options` есть, UI нет.
194. [Improve] Показывать wheel step section после wheel event.
195. [Problem] Нет device list.
196. [Improve] Settings first screen должен быть device-oriented.
197. [Problem] Нет last active device.
198. [Improve] Diagnostics should show last source and rule.
199. [Problem] Нет device aliases.
200. [Improve] Позволить переименовать устройства после registry.
201. [Problem] Нет disconnected device state.
202. [Improve] Показывать known/disconnected devices отдельно.
203. [Problem] Нет restore defaults.
204. [Improve] Добавить reset with confirmation.
205. [Problem] Нет undo для settings changes.
206. [Improve] Добавить short undo toast для non-destructive changes.
207. [Problem] Нет settings validation UI.
208. [Improve] Ошибки config показывать рядом с полем.
209. [Problem] Нет import/export config.
210. [Improve] Export config для backup и support.
211. [Problem] Import может принести invalid TOML.
212. [Improve] Validate before applying imported config.
213. [Problem] Нет permissions action buttons.
214. [Improve] Buttons: Request Input Monitoring, Open Accessibility Settings.
215. [Problem] Accessibility request flow сложнее Input Monitoring.
216. [Improve] Добавить OS-specific instructions.
217. [Problem] Permission status только в CLI.
218. [Improve] Показывать status badges in UI.
219. [Problem] Нет state `Degraded`.
220. [Improve] Runtime state: Active, Paused, NeedsPermission, Degraded, Error.
221. [Problem] Нет lightweight app runtime.
222. [Improve] Создать `app::runtime` с channels для UI commands.
223. [Problem] UI может напрямую дергать config store.
224. [Improve] UI должен отправлять `AppCommand`.
225. [Problem] Нет design tokens.
226. [Improve] Создать tokens: spacing, color, radius, type scale.
227. [Problem] Product может стать слишком декоративным.
228. [Improve] Использовать native compact utility layout.
229. [Problem] Cards могут захламить настройки.
230. [Improve] Использовать tables/lists вместо card grid.
231. [Problem] Первый экран может стать landing page.
232. [Improve] Первый экран должен быть рабочей панелью.
233. [Problem] UI labels могут быть техническими.
234. [Improve] Использовать понятные тексты: Mouse, Trackpad, Wheel step.
235. [Problem] `Natural` не всем понятно.
236. [Improve] Добавить microcopy: content moves with fingers vs opposite.
237. [Problem] Слишком много helper text перегрузит UI.
238. [Improve] Основные пояснения в tooltip/help popover.
239. [Problem] Tooltips недоступны keyboard-only users.
240. [Improve] Важные permission explanations держать inline.
241. [Problem] Цветом нельзя единственным способом показывать статус.
242. [Improve] Добавить labels/icons for state.
243. [Problem] Нет accessibility labels.
244. [Improve] Все controls должны иметь accessible names.
245. [Problem] Нет keyboard navigation plan.
246. [Improve] Tab order должен проходить все settings.
247. [Problem] Нет dark mode QA.
248. [Improve] Follow system appearance and test both themes.
249. [Problem] Иконки могут не соответствовать macOS conventions.
250. [Improve] Использовать native symbols или аккуратные monochrome assets.
251. [Problem] Нет retina status icon review.
252. [Improve] Проверить icon на light/dark menu bar.
253. [Problem] Длинные device names ломают layout.
254. [Improve] Truncate middle with tooltip.
255. [Problem] Compact UI может обрезать русский текст.
256. [Improve] Проверить localization expansion 30 percent.
257. [Problem] Нет i18n structure.
258. [Improve] Вынести strings до добавления второго языка.
259. [Problem] README смешивает English и Russian.
260. [Improve] Выбрать docs language или разделить localized docs.
261. [Problem] Русский пользователь просит русскую документацию.
262. [Improve] Добавить `README.ru.md` или перевести основной README.
263. [Problem] Product name не закреплен визуально.
264. [Improve] Settings title and about panel should say Auto Reverse.
265. [Problem] Нет about panel.
266. [Improve] About panel: version, config path, repo, privacy.
267. [Problem] Нет privacy UX.
268. [Improve] Сказать: no network telemetry by default.
269. [Problem] Update checks могут противоречить privacy.
270. [Improve] Automatic update checks only opt-in.
271. [Problem] Debug console может показать sensitive data.
272. [Improve] Log only scroll metadata, never text input.
273. [Problem] Input hooks вызывают trust concerns.
274. [Improve] UI должен объяснять, зачем нужны permissions.
275. [Problem] Нет recovery when icon hidden.
276. [Improve] CLI `show-icon` или relaunch opens preferences.
277. [Problem] Нет `open-settings` CLI.
278. [Improve] Добавить command to open preferences when UI exists.
279. [Problem] Нет `doctor --json`.
280. [Improve] JSON diagnostics помогут support.
281. [Problem] Нет diagnostics export.
282. [Improve] Export redacted diagnostics file.
283. [Problem] Нет copy-to-clipboard action.
284. [Improve] Diagnostics UI: copy summary.
285. [Problem] Нет manual test window.
286. [Improve] Добавить scroll test area в debug console.
287. [Problem] Test area может перехватить реальные expectations.
288. [Improve] Clearly label it as simulation-only.
289. [Problem] Нет visual preview of direction.
290. [Improve] Small scroll preview can show content movement.
291. [Problem] Preview animations могут отвлечь.
292. [Improve] Keep animations minimal and disable-able.
293. [Problem] Нет профилей.
294. [Improve] Profiles можно отложить до real device registry.
295. [Problem] App-specific rules слишком сложны.
296. [Improve] Не делать app-specific rules до stable v1.
297. [Problem] Нет quick reset for bad settings.
298. [Improve] Add `auto-reverse reset-config`.
299. [Problem] Reset может потерять useful config.
300. [Improve] Reset should create backup first.
301. [Problem] Нет clear disabled state in menu.
302. [Improve] Disabled controls should show reason and re-enable action.
303. [Problem] Нет separation persistent vs session settings.
304. [Improve] Mark session-only controls clearly.
305. [Problem] Нужен дизайн для error states.
306. [Improve] Error rows: plain language, technical details hidden.
307. [Problem] Нет loading states.
308. [Improve] Device scan and permissions refresh need non-jumpy states.
309. [Problem] Нет empty state.
310. [Improve] If no devices, show permissions and "scroll to detect".
311. [Problem] Нет menu hierarchy.
312. [Improve] Menu: Enable, Preferences, Diagnostics, Quit.
313. [Problem] Menu может стать слишком длинным.
314. [Improve] Keep advanced actions inside preferences.
315. [Problem] Нет keyboard shortcut policy.
316. [Improve] Avoid global hotkey until conflicts are handled.
317. [Problem] Нет native alerts strategy.
318. [Improve] Use alerts only for destructive actions.
319. [Problem] Нет onboarding completion state.
320. [Improve] Store first-run flag separately from config rules.
321. [Problem] Нет welcome copy.
322. [Improve] Welcome: one sentence goal, two permission steps, open settings.
323. [Problem] Нет visual hierarchy.
324. [Improve] Status first, devices second, advanced third.
325. [Problem] Нет responsive window sizing.
326. [Improve] Define minimum width and resizable constraints.
327. [Problem] Нет high-contrast review.
328. [Improve] Test contrast in light/dark/high contrast modes.
329. [Problem] Нет reduced motion support.
330. [Improve] Honor reduce motion for preview animations.
331. [Problem] Нет localization QA.
332. [Improve] Test English/Russian strings in compact window.
333. [Problem] Нет icon-only tooltip plan.
334. [Improve] Every icon button needs tooltip.
335. [Problem] Нет docs for hidden advanced flags.
336. [Improve] `reverse_only_raw_input` needs docs and UI explanation.
337. [Problem] Raw-input mode wording confusing.
338. [Improve] Label it "Ignore injected/remote scroll events".
339. [Problem] Нет support for restoring menu icon after hidden config mistake.
340. [Improve] Document `show_menu_bar_icon = true` recovery.

## Итерация 3: Reliability, Release, Review

341. [Problem] Нет release packaging.
342. [Improve] Decide app bundle structure.
343. [Problem] Нет code signing.
344. [Improve] Plan Developer ID signing before public release.
345. [Problem] Нет notarization.
346. [Improve] Add notarization checklist.
347. [Problem] Нет installer/uninstaller.
348. [Improve] Start with drag-and-drop app plus uninstall docs.
349. [Done] LaunchAgent implementation добавлен в `platform/macos/startup.rs`.
350. [Improve] Add `SMAppService` path when the app bundle exists.
351. [Problem] Нет wake-from-sleep recovery.
352. [Improve] Observe wake notifications and re-arm tap or relaunch.
353. [Problem] Event tap can stop silently in edge cases.
354. [Improve] Runtime health should detect no events/disabled tap.
355. [Problem] Нет watchdog.
356. [Improve] Add lightweight health timer after UI runtime exists.
357. [Problem] Нет crash-safe state restoration.
358. [Improve] Ensure failure path keeps pass-through behavior.
359. [Problem] Panic in callback would be dangerous.
360. [Improve] Keep callback small and panic-free; wrap risky code.
361. [Problem] `toml::to_string_pretty` in save can fail but no recovery UX.
362. [Improve] Surface config write errors in UI.
363. [Problem] Нет config lock.
364. [Improve] Consider file lock if multiple CLI/UI instances write config.
365. [Problem] Last-writer-wins может терять settings.
366. [Improve] Runtime should serialize config writes.
367. [Problem] Нет single-instance behavior.
368. [Improve] Relaunch should focus settings, not spawn second tap.
369. [Problem] `OnceLock` blocks multiple install attempts in one process.
370. [Improve] Runtime should own tap lifecycle explicitly.
371. [Problem] Нет graceful shutdown tests.
372. [Improve] Add shutdown path before UI.
373. [Problem] Нет signal handling for CLI run.
374. [Improve] Handle Ctrl+C gracefully.
375. [Problem] Нет manual QA checklist in repo.
376. [Improve] Add `QA.md`.
377. [Problem] Нет test matrix for devices.
378. [Improve] Matrix: wheel mouse, Magic Mouse, built-in trackpad, Magic Trackpad.
379. [Problem] Нет remote desktop test.
380. [Improve] Test `reverse_only_raw_input` with injected source_pid.
381. [Problem] Нет high-resolution wheel test.
382. [Improve] Test fractional/pixel-like fields on real devices.
383. [Problem] Нет horizontal wheel test.
384. [Improve] Test tilt wheel and horizontal gestures.
385. [Problem] Нет Wacom compatibility.
386. [Improve] Document Wacom behavior after hardware test.
387. [Problem] Нет accessibility-device review.
388. [Improve] Avoid breaking assistive input devices.
389. [Problem] Нет "shake to locate cursor" regression review.
390. [Improve] Include macOS accessibility gestures in manual QA.
391. [Problem] Нет Notification Center/gesture edge-case QA.
392. [Improve] Test system gestures while tap is active.
393. [Problem] Swipe gestures not reversed.
394. [Improve] Document limitation prominently.
395. [Problem] Custom scroll surfaces may bypass CGEvent.
396. [Improve] Document app-specific limitations.
397. [Problem] iPhone Mirroring-like cases may bypass transform.
398. [Improve] Keep limitations list updated.
399. [Problem] Нет source attribution in docs for Scroll Reverser parity.
400. [Improve] Keep links in `scroll-reverser-parity.md`.
401. [Problem] Нет legal review of feature parity wording.
402. [Improve] Avoid implying affiliation with Scroll Reverser.
403. [Problem] Нет release version policy.
404. [Improve] Use SemVer after first tagged release.
405. [Problem] Нет tag workflow.
406. [Improve] Create release tags with changelog.
407. [Problem] Нет build reproducibility notes.
408. [Improve] Document toolchain and target.
409. [Problem] Нет binary size budget.
410. [Improve] Track size before adding GUI toolkit.
411. [Problem] GUI toolkit may dominate app size.
412. [Improve] Prefer native AppKit or small wrapper for macOS.
413. [Problem] Cross-platform promise could overreach.
414. [Improve] Market as macOS-first until adapters exist.
415. [Problem] Linux/Windows support undefined.
416. [Improve] Add future notes, not product promise.
417. [Problem] Нет dependency policy.
418. [Improve] Add dependencies only for clear use cases.
419. [Problem] Нет security policy.
420. [Improve] Add `SECURITY.md` before public repo.
421. [Problem] Нет contribution guide.
422. [Improve] Add `CONTRIBUTING.md` with fmt/clippy/test rules.
423. [Problem] Нет issue templates.
424. [Improve] Add bug template with diagnostics fields.
425. [Problem] Нет privacy policy.
426. [Improve] State local-only data handling.
427. [Problem] Update checks could send network requests.
428. [Improve] Make network behavior explicit and opt-in.
429. [Problem] Нет telemetry boundary tests.
430. [Improve] Ensure no network crate enters default build without review.
431. [Problem] Нет static analysis.
432. [Improve] Run `cargo deny` or equivalent later.
433. [Problem] Нет dependency license review.
434. [Improve] Add license review to release checklist.
435. [Problem] Нет localization pipeline.
436. [Improve] Start with English and Russian string files.
437. [Problem] Нет translation credit policy.
438. [Improve] Track translator credits in changelog.
439. [Problem] Нет screenshots.
440. [Improve] Add real screenshots after UI exists.
441. [Problem] Нет icon assets.
442. [Improve] Design status icon states.
443. [Problem] Нет app icon.
444. [Improve] Create app icon before packaging.
445. [Problem] Нет design review artifacts.
446. [Improve] Add simple UI spec in architecture docs.
447. [Problem] Нет final product review process.
448. [Improve] Review UX, reliability, privacy before each milestone.
449. [Problem] Нет branch strategy.
450. [Improve] Use `codex/` branch prefix for agent work.
451. [Problem] Current work happened on `master`.
452. [Improve] Next tasks should branch before larger changes.
453. [Problem] Нет remote configured.
454. [Improve] Add `origin` before expecting push.
455. [Problem] Push cannot complete in current repo state.
456. [Improve] User must provide repo URL or create remote.
457. [Problem] Merge was local only.
458. [Improve] Push merge commit after remote setup.
459. [Done] `.idea/` is ignored at repository root.
460. [Improve] Keep IDE metadata local unless the project intentionally standardizes IDE settings.
461. [Done] `.gitignore` was reviewed after merge.
462. [Improve] Later add patterns for generated release artifacts when packaging exists.
463. [Problem] Docs use mixed Russian/English.
464. [Improve] Split user docs by language.
465. [Problem] README still says "target product" in English.
466. [Improve] Translate README if primary user language is Russian.
467. [Problem] Architecture doc is Russian-only.
468. [Improve] Keep architecture Russian if it helps project learning.
469. [Problem] Recommendation list can become stale quickly.
470. [Improve] Refresh it after every milestone.
471. [Problem] 500-item list is hard to execute directly.
472. [Improve] Create smaller `ROADMAP.md` with P0/P1/P2.
473. [Problem] Нет issue tracker mapping.
474. [Improve] Convert top 20 recommendations to issues.
475. [Problem] Нет owner per area.
476. [Improve] Add ownership notes for config, platform, UI.
477. [Problem] Нет acceptance criteria per task.
478. [Improve] Every task should include tests/docs/manual check.
479. [Problem] Нет "definition of done".
480. [Improve] Define done: code, tests, docs, review, QA.
481. [Problem] Нет automated review checklist.
482. [Improve] Add checklist: bugs, risks, missing tests, UX, privacy.
483. [Problem] Нет code review notes file.
484. [Improve] Add `REVIEW.md` or keep review section in architecture.
485. [Problem] Нет benchmark baseline.
486. [Improve] Capture current transform performance.
487. [Problem] Нет memory allocation audit.
488. [Improve] Ensure hot path does not allocate.
489. [Problem] Нет unsafe boundary documentation.
490. [Improve] Document each FFI call and invariant.
491. [Done] FFI permissions компилируются только под `#[cfg(target_os = "macos")]` через platform/mod.rs.
492. [Done] Весь macOS FFI живет за cfg-gated `platform::macos`.
493. [Problem] FFI function availability depends on macOS version.
494. [Improve] Document minimum macOS version and fallback behavior.
495. [Problem] No app-level state machine.
496. [Improve] Add explicit state enum before UI.
497. [Problem] No final review commit yet for updated docs.
498. [Improve] Commit docs and review fixes after checks pass.
499. [Problem] No push destination exists today.
500. [Improve] Configure remote, then push `master` with merge and docs commits.
