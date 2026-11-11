# Установка в пользовательский профиль

Home Manager автоматически подключает генерацию тем через hook `myt`.
Для установки без Home Manager можно использовать пользовательский Nix-профиль:

```sh
nix profile add github:Kostr2007/Terminal-new-MY
# Последующие обновления:
nix profile upgrade Terminal-new-MY
```

Подключите fish-интеграцию и дополнения из
`~/.nix-profile/share/terminal-new-my/tnmy.fish` и
`~/.nix-profile/share/fish/vendor_completions.d/tnmy.fish`.

Если `myt` уже управляется отдельной конфигурацией, автоматическую генерацию
можно подключить пользовательским systemd path unit. Для стандартных XDG-путей:

`~/.config/systemd/user/terminal-new-my-themes.service`:

```ini
[Unit]
Description=Generate Terminal New MY tool themes

[Service]
Type=oneshot
ExecStart=%h/.nix-profile/bin/tnmy --palette %h/.local/state/material-you-terminal/theme/palette.json theme --export %h/.local/state/terminal-new-my/tools --build-bat
```

`~/.config/systemd/user/terminal-new-my-themes.path`:

```ini
[Unit]
Description=Follow the Material You terminal palette

[Path]
PathChanged=%h/.local/state/material-you-terminal/theme/palette.json
Unit=terminal-new-my-themes.service

[Install]
WantedBy=default.target
```

```sh
systemctl --user daemon-reload
systemctl --user enable --now terminal-new-my-themes.path
systemctl --user start terminal-new-my-themes.service
```

При нестандартных XDG-путях или статической теме замените пути на значения
из `tnmy doctor --json`. Path unit следит за файлом, включая его атомарную замену;
он не перечитывает конфигурацию `myt`, если `themeDir` впоследствии изменился.
Используйте один механизм автоматического обновления: hook или path unit.

Отключение самостоятельной установки:

```sh
systemctl --user disable --now terminal-new-my-themes.path
rm ~/.config/systemd/user/terminal-new-my-themes.path
rm ~/.config/systemd/user/terminal-new-my-themes.service
systemctl --user daemon-reload
nix profile remove Terminal-new-MY
```

Удалите также созданный вами fish conf.d-файл интеграции. Экспортированные темы
в пользовательском состоянии можно сохранить или удалить отдельно.
