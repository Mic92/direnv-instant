{
  pkgs,
  lib,
  config,
  options,
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

  inherit (lib.hm.shell)
    mkBashIntegrationOption
    mkFishIntegrationOption
    mkZshIntegrationOption
    ;

  inherit (lib.types)
    int
    nullOr
    package
    str
    ;

  # On stable home-manager (<= 24.11), enableFishIntegration is read-only
  # because direnv ships fish vendor functions that auto-load.
  # On unstable home-manager, this was fixed and we can disable it.
  # See: https://github.com/Mic92/direnv-instant/issues/35
  fishIntegrationReadOnly = options.programs.direnv.enableFishIntegration.readOnly or false;
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

    enableBashIntegration = mkBashIntegrationOption { inherit config; };
    enableFishIntegration = mkFishIntegrationOption { inherit config; };
    enableZshIntegration = mkZshIntegrationOption { inherit config; };

    enableKittyIntegration = (mkEnableOption "kitty integration") // {
      default = config.programs.kitty.enable;
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
        enableBashIntegration = lib.mkIf cfg.enableBashIntegration (lib.mkForce false);
        enableZshIntegration = lib.mkIf cfg.enableZshIntegration (lib.mkForce false);
      }
      // lib.optionalAttrs (!fishIntegrationReadOnly) {
        # Only disable fish integration if the option is not read-only (unstable home-manager)
        # On stable home-manager (<= 25.11), both direnv and direnv-instant hooks run for fish.
        enableFishIntegration = lib.mkIf cfg.enableFishIntegration (lib.mkForce false);
      };

      home.packages = [ finalPackage ];

      programs.bash.initExtra = mkIf cfg.enableBashIntegration ''
        eval "$(direnv-instant hook bash)"
      '';

      programs.zsh.initContent = mkIf cfg.enableZshIntegration ''
        eval "$(direnv-instant hook zsh)"
      '';

      programs.fish.interactiveShellInit = mkIf cfg.enableFishIntegration ''
        direnv-instant hook fish | source
      '';

      programs.kitty.settings = mkIf cfg.enableKittyIntegration {
        allow_remote_control = true;
        listen_on = ''unix:kitty-{kitty_pid}'';
      };
    };
}
