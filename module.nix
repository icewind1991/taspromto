{ config
, lib
, pkgs
, ...
}:
with lib; let
  cfg = config.services.taspromto;
  format = pkgs.formats.toml { };
  configFile = format.generate "taspromto-config.toml" {
    listen = {
      inherit (cfg) port;
    };
    names = {
      mitemp = cfg.mitempNames;
      rftemp = cfg.rfChannelNames;
    };
    mqtt = {
      inherit (cfg.mqtt) hostname port;
      password_file = "$CREDENTIALS_DIRECTORY/mqtt_password";
    } // (
      optionalAttrs (cfg.mqtt.passwordFile != null) {
        inherit (cfg.mqtt) username;
        password_file = "$CREDENTIALS_DIRECTORY/mqtt_password";
      }
    );
  };
in
{
  options.services.taspromto = {
    enable = mkEnableOption "taspromto";

    mitempNames = mkOption {
      type = types.attrs;
      default = { };
      description = "Names for mitemp sensors";
    };

    rfChannelNames = mkOption {
      type = types.attrs;
      default = { };
      description = "Names for 433mhz temperature sensors";
    };

    port = mkOption {
      type = types.port;
      default = 3030;
      description = "port to listen to";
    };

    mqtt = mkOption {
      type = types.submodule {
        options = {
          hostname = mkOption {
            type = types.str;
            description = "Hostname of the MQTT server";
          };
          port = mkOption {
            type = types.port;
            default = 1883;
            description = "Port of the MQTT server";
          };
          username = mkOption {
            type = types.nullOr types.str;
            default = null;
            description = "Username for the MQTT server";
          };
          passwordFile = mkOption {
            type = types.nullOr types.str;
            default = null;
            description = "File containing the password for the MQTT server";
          };
        };
      };
    };

    package = mkOption {
      type = types.package;
      defaultText = literalExpression "pkgs.taspromto";
      description = "package to use";
    };
  };

  config = mkIf cfg.enable {
    systemd.services."taspromto" = {
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        LoadCredential = optional (cfg.mqtt.passwordFile != null) [
          "mqtt_password:${cfg.mqtt.passwordFile}"
        ];

        ExecStart = "${cfg.package}/bin/taspromto ${configFile}";

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
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
        RuntimeDirectory = "taspromto";
        RestrictRealtime = true;
        ProtectProc = "noaccess";
        SystemCallFilter = [ "@system-service" "~@resources" "~@privileged" ];
        IPAddressDeny = "any";
        IPAddressAllow = [ "localhost" cfg.mqtt.hostname ];
        PrivateUsers = true;
        ProcSubset = "pid";
      };
    };
  };
}
