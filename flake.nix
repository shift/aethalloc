{
  description = "AethAlloc - High-performance memory allocator for network workloads";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    let
      nixosModule = { config, lib, pkgs, ... }:
        let
          cfg = config.services.aethalloc;
          aethallocLib = "${cfg.package}/lib/libaethalloc.so";
        in
        {
          options.services.aethalloc = {
            enable = lib.mkEnableOption "AethAlloc memory allocator injection";
            
            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.default;
              description = "AethAlloc package to use";
            };
            
            services = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [];
              example = [ "systemd-networkd" "nftables" "suricata" ];
              description = "Systemd services to inject AethAlloc into";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.services = lib.genAttrs cfg.services (name: {
              environment.LD_PRELOAD = aethallocLib;
            });
            
            environment.systemPackages = [ cfg.package ];
          };
        };
    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        
        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [ "rust-src" ];
        };

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        aethalloc-rs = rustPlatform.buildRustPackage {
          pname = "aethalloc";
          version = "0.1.0";
          
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          buildInputs = with pkgs; [ numactl hwloc ];

          RUSTFLAGS = "-C target-cpu=native -C opt-level=3 -C link-arg=-Wl,-z,now";
          
          buildAndTestSubdir = "aethalloc-abi";
          
          installPhase = ''
            mkdir -p $out/lib
            cp target/x86_64-unknown-linux-gnu/release/libaethalloc_abi.so $out/lib/libaethalloc.so
          '';
        };

        withAethAlloc = pkg: pkgs.symlinkJoin {
          name = "${pkg.name}-aethalloc-wrapped";
          paths = [ pkg ];
          buildInputs = [ pkgs.makeWrapper ];
          postBuild = ''
            wrapProgram $out/bin/${pkg.meta.mainProgram or pkg.pname} \
              --set LD_PRELOAD "${aethalloc-rs}/lib/libaethalloc.so"
          '';
        };

      in {
        packages = {
          default = aethalloc-rs;
          suricata-aeth = withAethAlloc pkgs.suricata;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-flamegraph
            pkgs.valgrind
          ];
        };
      }
    ) // {
      nixosModules.default = nixosModule;
    };
}
