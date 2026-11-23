# Удаление платформенных функций Warp

## Зафиксированные источники и объём проверки

Дата исследования — 2026-11-06. Основная ревизия — `warpdotdev/warp@f7fc2f2a08a3aa446baa23960710e630c5defd6e`. Это исходный pin проекта. Проверены исходники и манифесты через GitHub, локальные копии использованы для поиска символов. Результаты сборки и испытаний фиксируются отдельно в [FOUNDATION.md](FOUNDATION.md).

Доноры рассмотрены по `the1812/warp-offline@f8d6106644067b19f24864f6b7c1c3aa59e5a327` и `terzigolu/warp-lite@a33c6debb0e9aed8a8abedfe4d325741050a62ec`. У первого последний рассмотренный commit датирован 30 апреля 2026 года. Перенос требует адаптации к текущему ноябрьскому upstream. [Исходный Warp](https://github.com/warpdotdev/warp/tree/f7fc2f2a08a3aa446baa23960710e630c5defd6e), [warp-offline](https://github.com/the1812/warp-offline/tree/f8d6106644067b19f24864f6b7c1c3aa59e5a327), [warp-lite](https://github.com/terzigolu/warp-lite/tree/a33c6debb0e9aed8a8abedfe4d325741050a62ec).

## Что уже установлено в основном upstream

### Граф сборки

- `app/Cargo.toml` объявляет пакет `warp`, библиотеку `app/src/lib.rs` и бинарник по умолчанию `warp-oss`. Остальные канальные бинарники используют ту же библиотеку. Изменение имени EXE само по себе оставляет общий startup graph.
- В манифесте приложения обязательны `ai`, `ai_types`, `cloud_object_client`, `cloud_object_models`, `cloud_objects`, `computer_use`, `mcp`, `oauth2`, `warp_server_auth`, `warp_server_client`, `warp_multi_agent_api`, `warp_multi_agent_client`, `rmcp`. Native-секция добавляет `cloud_object_persistence` и OpenTelemetry. `--no-default-features` сохраняет обязательные зависимости.
- В default feature set 208 элементов. Он смешивает терминальные возможности и платформу. Среди терминальных значений есть `settings_file`, `shell_selector`, `ligatures`, `rect_selection`, `kitty_images`, `kitty_keyboard_protocol`, `vertical_tabs`, `tab_configs`, `grouped_tabs`, `pinned_tabs`, `directory_tab_colors`, `terminal_lifecycle_recovery`, `ime_marked_text`, `history_search_ranking_v2`. Для терминального профиля нужен осмысленный список сохраняемых возможностей.
- `app/build.rs::add_features` на native вручную выдаёт `rustc-cfg` для `local_fs` и `local_tty`. В debug выдаёт также `agent_mode_debug`. Эти директивы активируют source cfg, сами по себе они не включают Cargo dependency features. Отдельно определение Cargo feature `local_fs` включает `ai/local_fs`, а `crash_reporting` и `jemalloc` также ссылаются на `ai`.
- Root workspace использует `members = ["crates/*", "app"]`, а `default-members` включает в том числе `graphql`. После очистки app graph потребуется отдельно привести workspace membership и оставшиеся manifests в соответствие с поставляемым деревом.

[Манифест приложения](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/Cargo.toml), [build.rs, add_features](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/build.rs#L231), [workspace](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/Cargo.toml).

### Первый перенос общего типа

`app/src/ai/blocklist/persistence.rs::SerializedBlockListItem` содержит единственный вариант `Command { block: Box<SerializedBlock> }`. Upstream TODO уже предлагает убрать wrapper. `AppState.block_lists` хранит этот тип, поэтому обычное восстановление терминальных блоков сейчас зависит от модуля `ai` по расположению типа.

Первый узкий патч может перенести тип и его conversions в терминальную persistence-область с обновлением импортов. Сначала сохранить его форму и поведение, чтобы не смешивать перенос с изменением сериализации. Это снимает реальную связанность и создаёт небольшой проверяемый diff. [Определение типа](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/ai/blocklist/persistence.rs#L584), [AppState](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/app_state.rs#L28).

### Snapshot и сохранение состояния

- `app/src/app_state.rs::TerminalPaneSnapshot` хранит нужные `uuid`, `cwd`, `shell_launch_data`, активность и read-only вместе с `InputConfig`, `llm_model_override`, `active_profile_id`, `conversation_ids_to_restore`, `active_conversation_id`.
- `InputConfig` находится в `app/src/ai/blocklist/input_model.rs`. У него `input_type` и `is_locked`, конструктор читает `AISettings`, а методы переключают Shell/AI. При полной очистке убрать AI-переключение из терминального ввода и определить чтение старого сохранённого значения. Переносить весь AI input model для двух полей не требуется.
- `WindowSnapshot` содержит локальную геометрию, вкладки и группы, а также `team_uid`, размеры AI/Drive/Voltron-панелей и agent management filters. `LeafContents` объединяет Terminal с Notebook, AIDocument, Workflow, AmbientAgent и другими панелями.
- `app/src/pane_group/pane/terminal_pane.rs::snapshot`, строки 467–569, при обычном локальном снимке обращается к `LLMPreferences`, `AIExecutionProfilesModel`, `BlocklistAIHistoryModel`, agent view controller. Простое прекращение регистрации этих singleton-моделей сломает создание снимка.
- `app/src/persistence/mod.rs::PersistedData` и `ModelEvent` объединяют историю/окна/блоки с cloud objects, профилями, experiment state, агентскими запросами, индексами, MCP и проектами.
- `app/src/persistence/sqlite.rs` записывает терминальную панель около строки 1251, читает около 2220. `read_sqlite_data` с 2487 обходит полноценную смешанную схему. `PersistedDataScope::Full` сохраняет окна, но также загружает AI/cloud. Готового terminal-only scope нет.

Сначала разъединить типы и callers, затем убирать singleton initialization и AI/cloud writers. Новый application ID/data namespace warp-ish обязателен уже для первого кандидата. Поведение локальной persistence должно сохранять последнюю известную CWD, переживать ранний startup snapshot и разрешать свежую оболочку в каталоге без повторного исполнения команд. При разработке миграций использовать копию тестовой базы и документированную версию схемы.

[Snapshot-типы](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/app_state.rs#L206), [InputConfig](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/ai/blocklist/input_model.rs#L120), [terminal snapshot](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/pane_group/pane/terminal_pane.rs#L467), [persistence types](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/persistence/mod.rs#L284), [SQLite](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/persistence/sqlite.rs#L2487).

### Запуск и локальный workspace

Главная точка композиции — `app/src/lib.rs::initialize_app`, примерно с 1495. Она строит auth/server providers, загружает persistence, регистрирует настройки, модели, действия и фоновые службы. Нужные терминальные зависимости включают PTY spawner, локальные preferences, GPU/appearance/fonts, history, window/workspace/terminal/input/editor actions, prompt, terminal keybindings и persistence writer.

Платформенные группы в этой функции включают `AuthState`/`AuthManager`, `ServerApiProvider`, `ServerExperiments`, `AIRequestUsageModel`, `UserWorkspaces`, API keys/subscriptions, `RemoteCodebaseIndexModel`, `TelemetryCollector`, `CloudEnvironmentCatalog`, agent conversation/permissions/notifications/orchestration models, `CloudPreferencesSyncer`, MCP managers/watchers, `AIDocumentModel`, `ScheduledAgentManager`, `LLMPreferences` и `InputClassifierModel`. Удаление выполняется после проверки всех `handle/as_ref` потребителей.

`RootView::new` в `app/src/root_view.rs` сейчас безусловно строит auth views и подписывается на `AuthManager`, `CloudPreferencesSyncer`, `UserWorkspaces`. Выбор первого UI определяется `AuthOnboardingState`, в том числе `ForceLogin`, `AgentOnboarding` и `SkipFirebaseAnonymousUser`. Целевой локальный startup строит `Workspace` напрямую из локального `NewWorkspaceSource`.

Событие `CloudPreferencesSyncerEvent::InitialLoadCompleted` задерживает применение onboarding settings. Оно не является общим условием восстановления окон. Восстановление окон идёт через `open_from_restored` и `GeneralSettings.restore_session`, начиная около 759. Эти пути нужно сохранить и освободить от auth/account context.

`app/src/app_services/mod.rs` отвечает за OS integration, включая open-terminal-here. `app/src/menu.rs` содержит общие UI menu primitives. Эти модули сохраняются по назначению. [Startup](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/lib.rs#L1495), [RootView](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/root_view.rs#L1884), [восстановление окон](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/root_view.rs#L759), [OS services](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/app_services/mod.rs).

### Настройки и оставшиеся входы

- `app/src/settings_view/mod.rs::SettingsSection` содержит Account, BillingAndUsage, WarpDrive, WarpAgent, AgentProfiles, AgentMCPServers, Knowledge, ThirdPartyCLIAgents, CodeIndexing, CloudEnvironments и WarpCloudAgentAPIKeys.
- `SettingsView::new` сначала создаёт page handles и подписки, затем собирает `settings_pages` и `nav_items`. Удаление только navigation rows сохраняет конструкторы страниц, actions, callbacks и backend consumers.
- `SettingsSection::from_slug`, `settings_widget_deeplink_target`, `SettingsAction`, `SettingsPageViewHandle` и поисковый индекс страниц также входят в объём очистки. Старые deep links не должны открывать удалённую страницу.
- `app/src/settings/init.rs::register_all_settings` регистрирует AISettings, CloudAgentSettings, CloudPreferencesSettings, WarpDrivePrivacySettings, SharedSessionSettings, WarpDriveSettings и другие группы отдельно от видимого UI. Это ключевая точка очистки машинной схемы и storage keys.
- `app/src/features.rs` связывает Cargo cfg с runtime feature flags. `ChannelState::additional_features`, release/debug flags и test overrides дают дополнительные пути включения. После полной excision соответствующие flags и handlers удаляются.
- `root_view:init` регистрирует `root_view:open_drive_object_new_window` и `root_view:open_drive_object_existing_window`. Нужен аудит действий, editable/fixed bindings, command palette, URI handlers, slash commands и contextual menus вместе с удалением соответствующих потребителей.

[Settings UI](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/settings_view/mod.rs#L1201), [регистрация групп](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/settings/init.rs#L59), [feature map](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/features.rs), [runtime flags](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/crates/warp_features/src/lib.rs#L1118).

### Нижележащие зависимости и сеть

`crates/warp_terminal/Cargo.toml` имеет обязательный `session-sharing-protocol`. В `crates/warp_terminal/src/model/block_id.rs` есть conversions между локальным BlockId и protocol BlockId/BufferId, а `src/shared_session.rs` содержит conversions для координат. Нужно убрать потребителей sharing и protocol conversions либо выделить необходимые локальные примитивы перед удалением зависимости. Сам `BlockId` нужен терминальному движку.

`warp_core` имеет прямые `reqwest` и `websocket` зависимости. Назначение каждого consumer нужно установить перед удалением transport crate. Для временной сетевой политики проверяются конкретные app-owned service paths. Наличие транспорта само по себе не показывает вызванный endpoint или работающий продуктовый путь.

Первый OSS entrypoint уже выставляет telemetry/crash/autoupdate config в `None`, но использует production `WarpServerConfig` и `OzConfig`. Для warp-ish нужен собственный channel configuration, application ID и data path. OMP остаётся обычным внешним процессом со своей текущей конфигурацией.

Инвентаризация loopback проводится по назначению. Локальные inference endpoints и встроенные агентские сервисы входят в удаление. Если shell integration, PTY/helper или app-local IPC действительно используют loopback, сохраняется конкретный нужный канал с проверкой инициатора. Общий allow-loopback из donor переносить как окончательную политику нельзя. [Terminal dependencies](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/crates/warp_terminal/Cargo.toml), [BlockId](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/crates/warp_terminal/src/model/block_id.rs#L49), [warp_core dependencies](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/crates/warp_core/Cargo.toml), [OSS entrypoint](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/app/src/bin/oss.rs#L10), [data paths](https://github.com/warpdotdev/warp/blob/f7fc2f2a08a3aa446baa23960710e630c5defd6e/crates/warp_core/src/paths.rs#L155).

## Конкретные решения donor warp-offline

### Что переносимо

1. `crates/network_policy/src/lib.rs` вводит `ServicesMode`, `NetworkPolicyDenied`, `check_url`/`check_url_str`, чистую `check_url_for_mode` и unit tests. Полезны явная ошибка, централизованная проверка перед app-owned запросом и тестируемость правила без сети.
2. `crates/warp_core/src/channel/config.rs` добавляет `services_mode`, `WarpServerConfig::local_only`, `OzConfig::local_only`. `channel/state.rs::set` передаёт режим в network_policy, а `is_local_only` используется вызывающим кодом. Это конкретный seam для раннего локального startup.
3. `app/src/bin/oss.rs` выбирает LocalOnly и отключённые ancillary configs. Перенести идею explicit terminal channel с собственным brand/data identity.
4. `crates/warp_features/src/online_services.rs` централизует список платформенных flags. Это готовая стартовая инвентаризация, которую нужно дополнить изменениями upstream после апреля.
5. `app/src/util/bindings.rs::CommandBinding::is_online_ai_or_agent_binding` и `bindings_tests.rs` показывают, как проверять недоступность агентских bindings. В donor есть исключение для Local AI command search. Для warp-ish оно исключается вместе с соответствующей реализацией.
6. Commit `cd5df19addb077b46cdfa2724ca4cc9ceb3d9fe3` содержит runtime guards в cloud sync queue/listener/update manager, preferences syncer, Drive, MCP, workflow/notebook/env var managers и settings. Он даёт готовый список стартующих потребителей.
7. Commit `5eea923eefe7279f9629d4bd1a2bfc649c37a01b` закрывает дополнительные входы в telemetry/autoupdate/server APIs, remote-server downloads, session sharing и CLI routes. Его file list удобен как checklist после первого запуска.

[network policy](https://github.com/the1812/warp-offline/blob/f8d6106644067b19f24864f6b7c1c3aa59e5a327/crates/network_policy/src/lib.rs), [channel config](https://github.com/the1812/warp-offline/blob/f8d6106644067b19f24864f6b7c1c3aa59e5a327/crates/warp_core/src/channel/config.rs), [channel state](https://github.com/the1812/warp-offline/blob/f8d6106644067b19f24864f6b7c1c3aa59e5a327/crates/warp_core/src/channel/state.rs), [feature inventory](https://github.com/the1812/warp-offline/blob/f8d6106644067b19f24864f6b7c1c3aa59e5a327/crates/warp_features/src/online_services.rs), [bindings](https://github.com/the1812/warp-offline/blob/f8d6106644067b19f24864f6b7c1c3aa59e5a327/app/src/util/bindings.rs#L791), [cloud guards commit](https://github.com/the1812/warp-offline/commit/cd5df19addb077b46cdfa2724ca4cc9ceb3d9fe3), [hardening commit](https://github.com/the1812/warp-offline/commit/5eea923eefe7279f9629d4bd1a2bfc649c37a01b).

### Ограничения переноса

В donor сохранён и поддерживается Local AI с OpenAI-compatible API. Policy разрешает весь loopback для HTTP/WS, а `local_only()` server configs указывают на `127.0.0.1`. Для warp-ish нужно удалить Local AI и broad loopback exception, предварительно инвентаризировав реальные app-local IPC consumers. Недостаточно подменить production URL на localhost.

Policy применяется только в местах, которые её вызывают. Она не перехватывает все сокеты процесса и не управляет дочерними пользовательскими программами. Unit tests проверки URL не заменяют процессную сетевую трассу.

`ServicesMode` меняет runtime поведение, сохраняя исходники, типы и Cargo зависимости AI/cloud. Это полезная промежуточная версия для сохранения рабочего терминала при последовательном разборе связей. Статус полной excision остаётся незавершённым. Автор сообщает только о тестировании на Windows 10, поэтому Windows 11 требует собственного прогона. [README donor](https://github.com/the1812/warp-offline/blob/f8d6106644067b19f24864f6b7c1c3aa59e5a327/README.md).

## Конкретные решения donor warp-lite

Основной переносимый commit — `6658d353848a8f458b5b87ce30297bf686b1f47f`, `feat(warp-lite): add platform boundaries and release benchmarks`.

- `app/Cargo.toml` вводит положительную compile feature `warp_platform = []`, исключённую из default Lite set.
- `app/src/lib.rs` ставит под cfg целые leaf modules `billing`, `pricing`, `reward_view`, связанные singleton registrations и ряд AI initialization/keybinding registrations. Граница также охватывает scheduled ambient-agent manager и agent status-bar tip.
- `app/src/settings_view/{mod.rs,settings_page.rs,main_page.rs,teams_page.rs}` убирают referral/billing constructors, page handles и зависимые CTA из default compilation. `app/src/workspace/{action.rs,mod.rs,view.rs}`, `resource_center/main_page.rs`, `referral_theme_status.rs` закрывают action variants, handlers, subscriptions и render paths того же функционала.
- `app/src/terminal/{mod.rs,input.rs,input/common.rs,input/agent.rs,input/universal.rs}` показывают узкие изменения полей, imports, construction и render calls для buy-credits UI. Терминальные renderer/input/model при этом сохраняются.
- `app/src/root_view.rs` в текущем donor проверяет `cfg!(feature = "skip_firebase_anonymous_user")` перед ForceLogin/onboarding ветками. Это обеспечивает loginless startup даже при изменении runtime flags. Такой механизм пригоден для промежуточного EXE. Финальная версия упрощает саму state machine до локального workspace.
- `script/benchmark-warp-lite.sh` и `script/benchmark-warp-lite-runtime.sh` полезны как примеры привязки evidence к ревизии, сравнения binary size и sampling процесса. Windows harness потребуется отдельный. В donor readiness checks частично являются proxy для появления процесса/окна.

[Boundary commit](https://github.com/terzigolu/warp-lite/commit/6658d353848a8f458b5b87ce30297bf686b1f47f), [manifest](https://github.com/terzigolu/warp-lite/blob/a33c6debb0e9aed8a8abedfe4d325741050a62ec/app/Cargo.toml#L423), [RootView](https://github.com/terzigolu/warp-lite/blob/a33c6debb0e9aed8a8abedfe4d325741050a62ec/app/src/root_view.rs#L1680), [технический отчёт](https://github.com/terzigolu/warp-lite/blob/a33c6debb0e9aed8a8abedfe4d325741050a62ec/specs/warp-lite-performance/TECH.md).

В donor прямо зафиксированы оставшиеся `ai`, `computer_use`, `onboarding`, `warp_graphql`, `warp_server_client`, `websocket`, `warp_managed_secrets` в resolved graph. `warp_platform = []` самостоятельно не делает эти dependencies optional. Сохраняются AI/auth source modules. Скрытые settings routes местами переводятся на Appearance, оставляя vocabulary старых sections. Эти решения относятся к промежуточной границе. Для итогового warp-ish требуются удаление реализаций и очистка registry/schema/dependencies.

README donor описывает macOS distribution и сохранение отдельных CLI-agent integrations. Текущая поставка warp-ish ограничена Windows 11 x64 и полным удалением встроенной платформы. Заимствованные решения проходят собственную проверку в этом объёме. [Состояние donor](https://github.com/terzigolu/warp-lite/blob/a33c6debb0e9aed8a8abedfe4d325741050a62ec/README.md), [оставшийся граф](https://github.com/terzigolu/warp-lite/blob/a33c6debb0e9aed8a8abedfe4d325741050a62ec/specs/warp-lite-performance/TECH.md#L97).

## Минимальная последовательность downstream

1. Сверить закреплённую базу и уже выполненные проверки из `FOUNDATION.md`. Использовать подтверждённые toolchain, LFS и команды сборки. Начать отделение общих типов и потребителей по реальным связям. Исходный GUI запускать для конкретного сравнения в выделенной Windows-среде. Целевые регрессии выполнять после затрагивающих их изменений.
2. Перенести узкие seam patterns из donor с сохранением provenance. Собственный channel/app ID/data path, принудительный локальный вход, отключение online service startup, runtime network checks по назначению, очистка первых UI/actions. Адаптировать отдельные hunks к f7fc2f2a. Крупные donor commits содержат несовместимые продуктовые решения и не годятся для слепого применения.
3. Для доступных leaf modules применить подход `warp_platform` как временную compile boundary либо сразу удалить соответствующую замкнутую ветку. Обрабатывать в одном изменении module declaration, import, field, constructor, subscription, action и render call. Начать с billing/referral/reward/pricing. После каждого среза компиляция и терминальный smoke.
4. Перенести `SerializedBlockListItem` и остальные реальные терминальные примитивы. Упростить snapshot, InputConfig и persistence read/write. Добавить конкретные регрессии для CWD, раннего startup snapshot и отсутствующего каталога на существующей тестовой базе.
5. Убрать AI/cloud consumers из terminal/workspace/root view. После этого удалить их singleton initialization, background workers, auth state machine и server APIs. Очистить settings registry, schema, deep links, action registry и hotkeys.
6. Удалить теперь неиспользуемые платформенные app modules/crates, их manifest edges, cfg/features, build-script cfg, assets и bundled helpers. Проверить `session-sharing-protocol`, lower-level shared primitives и transport consumers отдельно. Привести workspace и lockfile к выбранному терминальному составу.
7. Выполнить release gates на фактическом Windows пакете. Только после графа, source/UI/schema audits, persistence/TUI tests и сетевой трассы продвигать кандидат. Первый локальный EXE после шага 2 получает явный статус промежуточного.

Порядок срезов адаптируется к выявленным зависимостям. Работающий GUI/ConPTY наследуется из upstream, проверяется сохранение его поведения после удаления. Проверенный очищенный пакет передаётся пользователю, затем в рамках того же задания выполняется автоматическое сопровождение по `MAINTENANCE.md` и исторические переходы по `UPSTREAM.md`.

## Что автоматизировать уже сейчас

- Снимок pin и хешей затронутых файлов. Перед повторным применением патча сверять ожидаемый baseline. Изменение anchor или хеша отправляет кандидата на адаптацию без автоматического принятия широкого fuzzy match.
- Инвентаризацию root/app/native manifests, default features, обязательных платформенных deps и build-script cfg. Статический baseline должен показывать имеющиеся нарушения полноты удаления.
- Два отдельных отчёта для `cargo metadata` и `cargo tree`. Первый показывает workspace/source resolution, второй используется для реально выбранного package/target/features. Фиксировать build/dev зависимости отдельно от runtime. Наличие строки в Cargo.lock не доказывает включение в EXE.
- Проверку зарегистрированных settings groups, section slugs, action IDs, keybindings и startup models. Итоговый набор хранить как терминальный allowlist. Поиск слов AI/cloud служит вспомогательным сигналом, учитывая нейтральные термины и комментарии.
- Негативные проверки механизмов самой автоматики. В тестовой копии добавить известный запрещённый package/action/setting/service и убедиться, что release gate отклоняет кандидата.
- Выбор существующих upstream tests для `persistence/sqlite_tests.rs`, `launch_configs/launch_config_tests.rs`, `tab_configs/session_config_tests.rs` и терминальных моделей. Новые regression cases добавляются там, где конкретное изменение сохраняемых типов или CWD поведения не покрыто.
- Windows smoke harness со свежими PowerShell, TUI, resize, Ctrl+C, Unicode, multiline/paste, несколькими окнами и настоящим `Get-Location` в каждой восстановленной панели. Появление окна и наличие сохранённого cwd в базе недостаточны для успеха.
- Привязку runtime evidence к хешу фактического EXE. Сетевой прогон включает startup, idle, settings, restore и shutdown, проверяет app/helpers и отдельно разрешённую пользовательскую сетевую команду. Локальный inference также входит в наблюдение, app-local IPC классифицируется по назначению.

Перед применением каждого среза исполнитель сверяет эти привязки с исходным SHA кандидата. Результаты проверки записываются вместе с SHA патча и команды сборки.
