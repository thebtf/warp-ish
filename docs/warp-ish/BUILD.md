# Подготовка сборки на Windows

## Применимость

Прочитать при первой сборке, смене исходной ревизии, изменении toolchain или упаковки. Результаты исходной сборки записаны в [FOUNDATION.md](FOUNDATION.md).

## Исходная среда

Выполняйте команды PowerShell из корня checkout. Rust toolchain определяет `rust-toolchain.toml`, зависимости — `Cargo.lock`.

Выполнить обычную проверку доступности Rust, MSVC/Windows SDK и инструментов, требуемых `script/windows/bootstrap.ps1`. Устанавливать фактически недостающие зависимости в рамках разрешённого задания.

## Git LFS

Перед сборкой проверить LFS-пути в `.gitattributes` и доступность необходимых объектов выбранной ревизии. Стандартный механизм получения — `git lfs pull upstream`. Сохранить результат и конкретные недоступные объекты, если загрузка завершится ошибкой. Предварительную проверку доступности файлов выполняет `verify.py preflight`.

## Bootstrap исходного Warp

Bash-скрипт `script/bootstrap` по умолчанию предлагает установку common skills. У PowerShell-скрипта `script/windows/bootstrap.ps1` это отдельный switch `-InstallCommonSkills`. Для зависимостей форка использовать PowerShell-вход без этого switch и установить переменную только в процессе подготовки.

```powershell
$env:WARP_SKIP_COMMON_SKILLS_INSTALL = '1'
.\script\windows\bootstrap.ps1
```

Скрипт может устанавливать Rust, Build Tools, CMake, LLVM, Protobuf, InnoSetup и другие инструменты на компьютер. Перед выполнением проверить состав действий текущей ревизии и фактическую необходимость каждого компонента.

## Команды исходной сборки

Исполнитель выбирает применимые команды по актуальным манифестам и целевому бинарнику.

- `cargo check -p warp --bin warp-oss --locked -j 2` — проверка основного OSS-бинарника.
- `cargo build -p warp --bin warp-oss --release --locked -j 2` — кандидат релизной сборки до выбора собственного имени бинарника.
- `cargo nextest run -p <package>` — тесты затронутого пакета.
- `cargo clippy -p <package> --all-targets --tests -- -D warnings` — проверка изменённого пакета.
- `script/format` — форматирование по правилам текущего апстрима.
- `git diff --check` — проверка diff.

Повторяемая предварительная проверка из корня checkout

```powershell
python script/warp-ish/verify.py preflight --repo . --report .agent/runs/local/preflight.json
python -m unittest discover -s tests/warp-ish -p 'test_*.py' -v
```

Preflight записывает свободное место. Параметр `--min-free-gib` задаёт явный лимит для конкретного прогона. Потребность релизной сборки измеряется по свободному месту на томе Cargo и размеру его каталога артефактов до и во время прогона. Для базовой проверки использованы два параллельных задания и отдельный каталог артефактов.

```powershell
$env:RUSTUP_AUTO_INSTALL = '0'
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_TARGET_DIR = Join-Path $PWD '.agent/build/baseline'
New-Item -ItemType Directory -Path 'target/debug' -Force | Out-Null
cargo check -p warp --bin warp-oss --locked -j 2
```

В исходной ревизии build script помещает ConPTY и DXC assets в `target/debug` независимо от `CARGO_TARGET_DIR`. Первый прогон выявил отсутствие этого каталога. Команда подготовки выше учитывает это поведение для `cargo check`. При построении исполняемого пакета использовать согласованный каталог EXE и runtime assets.

После завершения прогона сохранить код выхода, лог и нужные артефакты приёмки. Удалить отработанный Cargo target после последней использующей его проверки, в том числе после ошибки сборки. Выполнять очистку только после остановки сборочных процессов, с проверенным `--target-dir` внутри checkout. Для указанного выше прогона очистить его отдельный target и созданный для него `target/debug`.

```powershell
cargo clean --target-dir .agent/build/baseline --locked --offline
cargo clean --target-dir target --profile dev --locked --offline
```

Логи в `.agent/runs` и готовые пакеты в `.agent/build/packages` находятся вне этих целей очистки.

Для первой поставки проверить release build scripts и упаковку выбранной ревизии. Собрать EXE, ConPTY/DXC и остальные требуемые runtime assets в полный пакет с собственным application ID. Проверить запуск EXE из этого пакета в тестовой среде и записать фактическую команду упаковки здесь. Результат `cargo check` служит проверкой кода, сведения о готовом пакете фиксируются отдельно.

Перед запуском GUI задать и проверить собственный каталог данных warp-ish. В текущем upstream `WARP_DATA_PROFILE` применяется только в debug-сборках. Проверка путей для релизного пакета описана в [TESTING.md](../../docs/warp-ish/TESTING.md). Полный `script/presubmit` выбирается по охвату изменения и запросу на выпуск. При смене имён бинарников и упаковки обновить этот справочник на проверенные команды форка.

## Локальный пакет Windows x64

После фиксации исходников в чистом checkout использовать PowerShell 7 из корня репозитория:

```powershell
.\script\warp-ish\package-windows.ps1 -Version '0.1.0-local.1'
```

Версия задаётся явно: три числовых компонента и необязательный суффикс через дефис. Повторная запись существующей версии запрещена. Изменённые tracked-файлы и неизвестные untracked-входы сборки блокируют упаковку. Исключены только untracked runtime-каталоги `.agent/continuity/`, `.agent/session-checkpoints/`, `.agent/tmp/`, `.serena/`; build/runs уже игнорируются Git. Требуется версия `cargo-about`, закреплённая в `script/install_cargo_release_deps`; недостающий инструмент подготовить до сборки. Inno Setup для локального ZIP не требуется.

Скрипт читает точную версию Rust из `rust-toolchain.toml`, явно выбирает её через `cargo +<version>` и `rustc +<version>` и проверяет ответы обоих инструментов. Переменная `RUSTUP_TOOLCHAIN` вызывающего процесса не меняет выбранную версию. Сборка выполняет `cargo +<version> build -p warp --bin warp-oss --profile rlto --features release_bundle,gui --target x86_64-pc-windows-msvc --locked -j 2` с отдельным каталогом Cargo. Скрипт проверяет чистоту исходников до и после сборки и перед продвижением пакета. Upstream SHA берётся из `terminal-policy.json` и должен быть предком fork SHA.

Результат находится в `.agent/build/packages/<version>/`: каталог `app/` с EXE и полным runtime layout, ZIP, `manifest.json` с исходной ревизией, параметрами сборки и хешами файлов, `package.json` с SHA256 архива и манифеста. Схема настроек генерируется командой `dump-settings-schema` именно из упакованного EXE. Лицензии и ресурсы готовит существующий Windows helper в режиме `TerminalOnly`.

Пакет продвигается переименованием нового каталога только после проверки полного состава ZIP и хешей его содержимого. Это локальный кандидат с `acceptance: NOT_RUN`; обязательные проверки выполняются из `app/` по `ACCEPTANCE.md`. Скрипт не устанавливает приложение и не меняет пользовательские данные.

Лог и `state.json` сохраняются в `.agent/runs/package-<version>-<id>/`. Отдельный Cargo target и принадлежащий этому прогону `target/rlto` очищаются после последнего использования, включая ошибку; существующий `target/rlto` вызывает отказ до сборки. Не запускать другие сборки в том же checkout одновременно. При ошибке скрипт сохраняет причину и хеши доступных staged-файлов, затем удаляет принадлежащий прогону `.incomplete` staging вместе с build targets. Сохраняются только успешно продвинутые кандидаты и сведения о прогоне. Если очистка отказывает или завершается ошибкой, `state.json` и ошибка процесса содержат точный оставшийся путь и причину; удаление в таком случае не объявляется успешным.

Проверка workflow без Cargo, установки и GUI использует stubs внешних процессов и настоящий helper ресурсов:

```powershell
.\tests\warp-ish\test_package_windows.ps1 -WrapperSource .\script\warp-ish\package-windows.ps1
```

## Результаты

Логи сборки, сведения о версии инструментов, исходный SHA и проверки пакета относятся к конкретному кандидату. Их ссылки записываются в текущем задании.
