# Terminal New MY. Source from an interactive fish session.
status is-interactive; or return

function myls --description 'Material You file list'
    command tnmy ls $argv
end

function myfiles --description 'Material You file browser'
    command tnmy files $argv
end

function myupdate --description 'Rebuild via nh; add --refresh to update inputs'
    command tnmy update $argv
end

# Reuse the already configured myt path, including immutable Nix store themes.
if not set -q TNMY_PALETTE; and set -q __myt_theme_dir
    set -gx TNMY_PALETTE "$__myt_theme_dir/palette.json"
end
