self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.terminal-new-my;
in
{
  options.programs.terminal-new-my = {
    enable = lib.mkEnableOption "Terminal New MY";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "The tnmy package.";
    };
    paletteFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Explicit palette.json; null discovers the existing myt configuration.";
    };
    enableFishIntegration = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Add myls, myfiles and myupdate functions without replacing existing commands.";
    };
    replaceLs = lib.mkEnableOption "replace interactive ls with tnmy ls (supports tnmy flags only)";
  };
  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];
    home.sessionVariables = lib.mkIf (cfg.paletteFile != null) { TNMY_PALETTE = cfg.paletteFile; };
    programs.fish = lib.mkIf cfg.enableFishIntegration {
      enable = lib.mkDefault true;
      interactiveShellInit = lib.mkAfter ''
        source ${cfg.package}/share/terminal-new-my/tnmy.fish
      '';
      shellAliases = lib.mkIf cfg.replaceLs { ls = lib.mkForce "tnmy ls"; };
    };
  };
}
