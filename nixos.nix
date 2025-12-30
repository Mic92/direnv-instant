{
  pkgs,
  lib,
  config,
  ...
}:
let
  cfg = config.programs.direnv-instant;

  inherit (lib)
    mkEnableOption
    mkIf
    mkOption
    optionalString
    ;

  inherit (lib.types)
    int
    nullOr
    package
    str
    ;
in
{
  options.programs.direnv-instant = {
    enable = mkEnableOption "non-blocking direnv integration daemon with tmux support";
    package = mkOption {
      type = package;
      default = pkgs.callPackage ./default.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./default.nix { }";
      description = "The direnv-instant package to use.";
    };
    finalPackage = mkOption {
      description = "Resulting direnv-instant package";
      type = package;
      readOnly = true;
      visible = false;
    };

    enableBashIntegration = mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to enable Bash integration.";
    };

    enableZshIntegration = mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to enable Zsh integration.";
    };

    enableFishIntegration = mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to enable Fish integration.";
    };

    settings = {
      use_cache = (mkEnableOption "cached environment loading for instant prompts") // {
        default = true;
      };
      mux_delay = mkOption {
        description = "Delay in seconds before spawning multiplexer pane";
        type = int;
        default = 4;
        example = 1;
      };
      debug_log = mkOption {
        description = "Path to debug log for daemon output";
        type = nullOr str;
        default = null;
        example = "/tmp/direnv-instant.log";
      };
    };
  };

  config =
    let
      finalPackage =
        pkgs.runCommand "direnv-instant-wrapped"
          {
            nativeBuildInputs = [ pkgs.makeWrapper ];
            inherit (cfg.package) meta;
          }
          ''
            mkdir -p $out/bin
            makeWrapper ${cfg.package}/bin/direnv-instant $out/bin/direnv-instant \
              --set-default DIRENV_INSTANT_USE_CACHE ${if cfg.settings.use_cache then "1" else "0"} \
              --set-default DIRENV_INSTANT_MUX_DELAY ${builtins.toString cfg.settings.mux_delay} \
              ${optionalString (
                cfg.settings.debug_log != null
              ) "--set-default DIRENV_INSTANT_DEBUG_LOG '${cfg.settings.debug_log}'"}
          '';
    in
    mkIf cfg.enable {
      programs.direnv-instant = { inherit finalPackage; };
      programs.direnv = {
        enable = lib.mkDefault true;
        # direnv and direnv-instant have mutually exclusive hooks
        enableBashIntegration = lib.mkForce (!cfg.enableBashIntegration);
        enableZshIntegration = lib.mkForce (!cfg.enableZshIntegration);
        enableFishIntegration = lib.mkForce (!cfg.enableFishIntegration);
      };

      environment.systemPackages = [ finalPackage ];

      programs.bash.interactiveShellInit = mkIf cfg.enableBashIntegration ''
        eval "$(direnv-instant hook bash)"
      '';

      programs.zsh.interactiveShellInit = mkIf cfg.enableZshIntegration ''
        eval "$(direnv-instant hook zsh)"
      '';

      programs.fish.interactiveShellInit = mkIf cfg.enableFishIntegration ''
        direnv-instant hook fish | source
      '';
    };
}
