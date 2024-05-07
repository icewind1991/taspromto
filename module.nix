{
  config,
  lib,
  pkgs,
  ...
}:
with lib; let
  cfg = config.services.taspromto;
in {
  options.services.taspromto = {
    enable = mkEnableOption "taspromto";

    mitempNames = mkOption {
      type = types.attrs;
      default = {};
      description = "Names for mitemp sensors";
    };

    rfChannelNames = mkOption {
      type = types.attrs;
      default = {};
      description = "Names for 433mhz temperature sensors";
    };

    port = mkOption {
      type = types.int;
      default = 3030;
      description = "port to listen to";
    };

    mqttCredentailsFile = mkOption {
      type = types.str;
      description = "path containing MQTT_HOSTNAME, MQTT_USERNAME and MQTT_PASSWORD environment variables";
    };

    package = mkOption {
      type = types.package;
      defaultText = literalExpression "pkgs.shelve";
      description = "package to use";
    };
  };

  config = mkIf cfg.enable {
    systemd.services."taspromto" = {
      wantedBy = ["multi-user.target"];
      environment = {
        PORT = toString cfg.port;
        MITEMP_NAMES = concatStringsSep "," (map (k: k + "=" + cfg.mitempNames."${k}") (attrNames cfg.mitempNames));
        RF_TEMP_NAMES = concatStringsSep "," (map (k: k + "=" + cfg.rfChannelNames."${k}") (attrNames cfg.rfChannelNames));
      };

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/taspromto";
        EnvironmentFile = cfg.mqttCredentailsFile;
        Restart = "on-failure";
        DynamicUser = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        ProtectClock = true;
        CapabilityBoundingSet = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        SystemCallArchitectures = "native";
        ProtectKernelModules = true;
        RestrictNamespaces = true;
        MemoryDenyWriteExecute = true;
        ProtectHostname = true;
        LockPersonality = true;
        ProtectKernelTunables = true;
        RestrictAddressFamilies = "AF_INET AF_INET6";
        RestrictRealtime = true;
        ProtectProc = "noaccess";
        SystemCallFilter = ["@system-service" "~@resources" "~@privileged"];
        IPAddressDeny = "any";
        IPAddressAllow = ["localhost"];
        PrivateUsers = true;
        ProcSubset = "pid";
      };
    };
  };
}
