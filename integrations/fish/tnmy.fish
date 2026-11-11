# Terminal New MY. Source from an interactive fish session.
status is-interactive; or return

function myls --wraps 'tnmy ls' --description 'Material You file list'
    command tnmy ls $argv
end

function myfiles --wraps 'tnmy files' --description 'Material You file browser'
    command tnmy files $argv
end

function myupdate --wraps 'tnmy update' --description 'Rebuild via nh; add --refresh to update inputs'
    command tnmy update $argv
end

function mycd --description 'Choose a directory in the Material file browser'
    set -l destination (command tnmy files --pick-directory $argv | string split0)
    if set -q destination[1]
        builtin cd -- "$destination[1]"
    end
end

function __tnmy_tools --description 'Locate the shared generated tool themes'
    set -l directory "$TNMY_TOOL_THEME_DIR"
    if test -z "$directory"
        set -l state "$XDG_STATE_HOME"
        test -n "$state"; or set state "$HOME/.local/state"
        set directory "$state/terminal-new-my/tools"
    end
    if not test -r "$directory/manifest.json"
        echo 'Run tnmy theme --export <directory> --build-bat first, or enable toolThemes in Home Manager.' >&2
        return 1
    end
    printf '%s\0' "$directory"
end

function myeza --wraps eza --description 'eza in the shared Material theme'
    set -l directory (__tnmy_tools | string split0); or return 1
    command env EZA_CONFIG_DIR="$directory/eza" eza $argv
end

function mybat --wraps bat --description 'bat in the shared Material theme'
    set -l directory (__tnmy_tools | string split0); or return 1
    command env BAT_CONFIG_DIR="$directory/bat" BAT_CACHE_PATH="$directory/bat-cache" BAT_THEME=TerminalNewMY bat $argv
end

function myfzf --wraps fzf --description 'fzf in the shared Material theme'
    set -l directory (__tnmy_tools | string split0); or return 1
    set -l color (string collect < "$directory/fzf.opts")
    command fzf "$color" $argv
end

function mydiff --wraps delta --description 'delta in the shared Material theme'
    set -l directory (__tnmy_tools | string split0); or return 1
    set -l options (string split0 < "$directory/delta.args")
    command env BAT_CONFIG_DIR="$directory/bat" BAT_CACHE_PATH="$directory/bat-cache" delta $options $argv
end

# Reuse the already configured myt path, including immutable Nix store themes.
if not set -q TNMY_PALETTE; and set -q __myt_theme_dir
    set -gx TNMY_PALETTE "$__myt_theme_dir/palette.json"
end
