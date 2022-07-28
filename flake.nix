{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    naersk,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages."${system}";
        naersk-lib = naersk.lib."${system}";
      in rec {
        # `nix build`
        packages.taspromto = naersk-lib.buildPackage {
          pname = "taspromto";
          root = ./.;
        };
        defaultPackage = packages.taspromto;
        defaultApp = packages.taspromto;

        # `nix develop`
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [rustc cargo bacon cargo-edit cargo-outdated];
        };
      }
    )
    // {
      nixosModule = {
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

            port = mkOption {
              type = types.int;
              default = 3030;
              description = "port to listen to";
            };

            mqttCredentailsFile = mkOption {
              type = types.str;
              description = "path containing MQTT_HOSTNAME, MQTT_USERNAME and MQTT_PASSWORD environment variables";
            };
          };

          config = mkIf cfg.enable {
            systemd.services."taspromto" = let
              pkg = self.defaultPackage.${pkgs.system};
            in {
              wantedBy = ["multi-user.target"];
              script = "${pkg}/bin/taspromto";
              environment = {
                PORT = toString cfg.port;
                MITEMP_NAMES = concatStringsSep "," (map (k: k + "=" + cfg.mitempNames."${k}") (attrNames cfg.mitempNames));
              };

              serviceConfig = {
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
        };
    };
}
