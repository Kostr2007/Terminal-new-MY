# Terminal New MY

Material You для повседневной работы в терминале: файлы, навигация и обновления
в общей палитре [Material-You-Terminal](https://github.com/Kostr2007/Material-You-Terminal).
Работает в [Kitty Material You](https://github.com/Kostr2007/Kitty-Material-You)
и обычных терминалах с UTF-8.

**v0.1 — первая рабочая версия.** Собственные нативные карточки Kitty, анимации
и полный экран событий Nix находятся в [дорожной карте](docs/roadmap.md).

![Файлы в тёмной Material-теме](docs/screenshots/files-dark.png)

<details>
<summary>Светлая тема и вывод списка</summary>

![Файлы в светлой Material-теме](docs/screenshots/files-light.png)
![Список файлов в истории терминала](docs/screenshots/list-dark.png)

Снимки реального Kitty с базовыми Material-палитрами. Промпт на снимке списка
принадлежит существующей конфигурации fish/Starship, а не новому приложению.

</details>

## Попробовать

```sh
nix run github:Kostr2007/Terminal-new-MY -- ls -la
nix run github:Kostr2007/Terminal-new-MY -- files
```

Или собрать локально (Rust 1.88+):

```sh
cargo build --release --locked
./target/release/tnmy ls -la
./target/release/tnmy files
./target/release/tnmy doctor
```

## Что работает

| Команда | Возможности |
| --- | --- |
| `tnmy ls [-la] [PATH…]` | Файлы и метаданные, каталоги первыми, Material-цвета |
| `tnmy ls --sort size` | Сортировка по имени, размеру или изменению |
| `tnmy ls --json` | Метаданные для программ |
| `tnmy ls --zero` | Исходные пути с NUL-разделителем |
| `tnmy files [PATH]` | Браузер с поиском и текстовым превью |
| `tnmy update` | Пересборка текущей конфигурации через `nh os switch` |
| `tnmy update --refresh` | Обновление входов flake и пересборка |
| `tnmy doctor [--json]` | Источник палитры и возможности окружения |
| `tnmy theme` | Цветовые роли текущей темы |

`tnmy ls` поддерживает собственные флаги; совместимость со всеми аргументами
GNU `ls` или `eza` не заявляется. Без цвета и при перенаправлении обычный список
содержит имена без оформления; длинные имена не обрезаются.
Для машинной обработки имён с переводами строки или не-UTF-8 байтами используйте
`--zero`. JSON требует UTF-8 путей. Не-UTF-8 имена в интерфейсе отображаются
с заменой нераспознаваемых байтов.

Браузер: **↑/↓** или **j/k** — выбор, **Enter/→** — каталог,
**←/Backspace** — родитель, **/** — поиск, **a** — скрытые файлы,
**s** — сортировка, **r** — обновление списка и темы, **q/Ctrl+C** — выход.
В поиске **Enter** завершает ввод, **Esc** очищает запрос.
Превью появляется при ширине от 90 колонок. Браузер требует интерактивный
stdin/stdout и не меняет рабочий каталог родительского shell.

## Существующая Material-тема

`tnmy` автоматически читает конфигурацию `myt`, включая `themeDir` в Nix store
или пользовательском состоянии. Если темы нет, используется встроенная тёмная.

```sh
tnmy --palette /path/to/palette.json ls
tnmy --color never files
tnmy --color always ls
```

Переменная `TNMY_PALETTE` задаёт путь явно. Также поддерживаются `MYT_PALETTE`,
`MYT_THEME_DIR` и `MYT_CONFIG`. `NO_COLOR` отключает автоматический цвет.
В браузере тема перечитывается при **r** и resize.

## Nix / Home Manager

Добавьте вход и импортируйте модуль в вашу конфигурацию Home Manager:

```nix
# flake.nix: inputs
terminal-new-my.url = "github:Kostr2007/Terminal-new-MY";
terminal-new-my.inputs.nixpkgs.follows = "nixpkgs";
```

```nix
# Модуль Home Manager, получающий inputs из вашей конфигурации
{ inputs, ... }: {
  imports = [ inputs.terminal-new-my.homeManagerModules.default ];
  programs.terminal-new-my.enable = true;
  # Необязательно:
  # programs.terminal-new-my.paletteFile = "/path/to/palette.json";
  # programs.terminal-new-my.replaceLs = true;
}
```

Модуль устанавливает `tnmy`, дополнения fish и функции `myls`, `myfiles`,
`myupdate`. Существующие промпт, приветствие, плагины и aliases сохраняются.
`replaceLs = true` явно заменяет fish alias `ls`; поддерживаются флаги `tnmy`.
Linux x86_64 проверяется локально и в CI; aarch64 output предусмотрен,
но на ARM пока не проверен.

Без Home Manager:

```fish
source /path/to/Terminal-new-MY/integrations/fish/tnmy.fish
source /path/to/Terminal-new-MY/integrations/fish/completions/tnmy.fish
```

Бинарник `tnmy` должен быть в `PATH`.

## Обновления системы

```sh
tnmy update --flake /etc/nixos --dry-run
tnmy update --flake /etc/nixos --action build
tnmy update --flake /etc/nixos --refresh
tnmy update --flake /etc/nixos -- --offline
```

При отсутствии `--flake` параметры источника берёт сам `nh` из своего окружения.
`--dry-run` печатает JSON-массив аргументов и ничего не запускает.
Текущая версия сохраняет интерфейс `nh`/`nom`, запросы привилегий и вывод
активации. Добавляются название операции, длительность и итог с кодом завершения.
Собственная визуализация событий сборки ещё не реализована.

## Разработка

```sh
nix develop
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tools/pty-check.py target/debug/tnmy
nix flake check --print-build-logs
```

PTY-проверки используют стандартный Python на Unix. Они проверяют навигацию,
поиск, узкие окна, FIFO, отсутствие перерисовки в покое и восстановление терминала.
Тесты обновлений используют подставной `nh` и не меняют систему.

[Архитектура и источники](docs/architecture.md) · [План развития](docs/roadmap.md)
