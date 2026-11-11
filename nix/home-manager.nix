self:
{
  config,
  lib,
  pkgs,
  options,
  ...
}:
let
  cfg = config.programs.terminal-new-my;
  toolDir = "${config.xdg.stateHome}/terminal-new-my/tools";
  exe = lib.getExe cfg.package;
  paletteArg = lib.optionalString (
    cfg.paletteFile != null
  ) "--palette ${lib.escapeShellArg cfg.paletteFile}";
  exportCommand = "${exe} ${paletteArg} theme --export ${lib.escapeShellArg toolDir} --build-bat";
  hasMyt = lib.hasAttrByPath [ "programs" "material-you-terminal" "dynamic" "hooks" ] options;
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
    density = lib.mkOption {
      type = lib.types.enum [
        "compact"
        "comfortable"
      ];
      default = "comfortable";
      description = "Default list and browser density.";
    };
    enableFishIntegration = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Add file browsing, directory picking and themed tool functions.";
    };
    toolThemes.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Generate eza, bat, fzf and delta themes, and refresh them after myt applies a palette.";
    };
    replaceLs = lib.mkEnableOption "replace interactive ls with tnmy ls (supports tnmy flags only)";
  };
  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        home.packages = [
          cfg.package
        ]
        ++ lib.optionals cfg.toolThemes.enable [
          pkgs.eza
          pkgs.bat
          pkgs.fzf
          pkgs.delta
        ];
        home.sessionVariables = {
          TNMY_DENSITY = cfg.density;
        }
        // lib.optionalAttrs (cfg.paletteFile != null) { TNMY_PALETTE = cfg.paletteFile; }
        // lib.optionalAttrs cfg.toolThemes.enable { TNMY_TOOL_THEME_DIR = toolDir; };
        home.activation.terminalNewMyThemes = lib.mkIf cfg.toolThemes.enable (
          lib.hm.dag.entryAfter [ "linkGeneration" "materialYouTerminal" ] ''
            run ${exportCommand}
          ''
        );
        programs.fish = lib.mkIf cfg.enableFishIntegration {
          enable = lib.mkDefault true;
          interactiveShellInit = lib.mkAfter ''
            source ${cfg.package}/share/terminal-new-my/tnmy.fish
          '';
          shellAliases = lib.mkIf cfg.replaceLs { ls = lib.mkForce "tnmy ls"; };
        };
      }
      (lib.optionalAttrs hasMyt {
        programs.material-you-terminal.dynamic.hooks = lib.mkIf cfg.toolThemes.enable [
          "${exe} --palette \"$MYT_PALETTE\" theme --export ${lib.escapeShellArg toolDir} --build-bat"
        ];
      })
    ]
  );
}
