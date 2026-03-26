{
  description = "siomon - Linux hardware information and real-time sensor monitoring tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      # siomon is Linux-only (reads /sys, /proc, ioctls, etc.)
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
    in
    flake-utils.lib.eachSystem supportedSystems (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # Factory function: build sinfo_io.ko against any kernel
        mkSinfoIoKmod = kernel: pkgs.stdenv.mkDerivation {
          pname = "sinfo-io";
          version = "0.1.0";
          src = ./kmod/sinfo_io;

          nativeBuildInputs = kernel.moduleBuildDependencies;

          makeFlags = [
            "KDIR=${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
          ];

          installPhase = ''
            runHook preInstall
            install -D sinfo_io.ko \
              "$out/lib/modules/${kernel.modDirVersion}/extra/sinfo_io.ko"
            runHook postInstall
          '';

          meta = with pkgs.lib; {
            description = "Kernel module for siomon atomic Super I/O register access";
            longDescription = ''
              Optional kernel module that provides a /dev/sinfo_io character device for
              atomic banked register reads on Nuvoton NCT67xx and ITE IT87xx Super I/O
              hardware monitoring chips, avoiding race conditions with the in-kernel
              nct6775/it87 hwmon drivers.
            '';
            license = licenses.gpl2Only;
            platforms = platforms.linux;
          };
        };
      in
      {
        # ── Packages ──────────────────────────────────────────────────────────
        packages = {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "siomon";
            version = "0.2.2";
            src = ./.;

            # Reads Cargo.lock directly; no cargoHash required.
            cargoLock.lockFile = ./Cargo.lock;

            meta = with pkgs.lib; {
              description = "Linux hardware information and real-time sensor monitoring";
              longDescription = ''
                siomon is a zero-runtime-dependency binary for Linux that reports
                detailed CPU, memory, GPU, storage, network, audio, USB, battery,
                PCIe, and SMBIOS/DMI hardware information.  It also offers an
                interactive TUI dashboard for real-time hwmon / RAPL / GPU sensor
                monitoring with configurable alerts and CSV logging.
                The binary is called `sio`.
              '';
              homepage = "https://github.com/level1techs/siomon";
              license = licenses.mit;
              maintainers = [ ];
              platforms = platforms.linux;
              mainProgram = "sio";
            };
          };

          # sinfo_io kernel module built against the default system kernel.
          # For other kernels use `packages.${system}.lib.mkSinfoIoKmod`.
          sinfo-io-kmod = mkSinfoIoKmod pkgs.linuxPackages.kernel;
        };

        # ── Library outputs ───────────────────────────────────────────────────
        # Expose the module factory so downstream flakes can build against any
        # kernel: `inputs.siomon.lib.x86_64-linux.mkSinfoIoKmod pkgs.linuxPackages_latest.kernel`
        lib.mkSinfoIoKmod = mkSinfoIoKmod;
      }
    )

    # ── System-independent outputs ─────────────────────────────────────────
    // {
      # Nixpkgs overlay: adds `pkgs.siomon`
      overlays.default = final: _prev: {
        siomon = self.packages.${final.stdenv.hostPlatform.system}.default;
      };
    };
}
