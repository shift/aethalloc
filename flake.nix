{
  description = "AethAlloc - Rust Architectural Implementation";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
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
          pname = "aethalloc-rs";
          version = "0.1.0";
          
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          buildInputs = with pkgs; [ numactl hwloc ];

          RUSTFLAGS = "-C target-cpu=native -C opt-level=3 -C link-arg=-Wl,-z,now";
          
          installPhase = ''
            mkdir -p $out/lib
            cp target/release/libaethalloc_abi.so $out/lib/libaethalloc.so
          '';
        };

        withAethAlloc = pkg: pkgs.symlinkJoin {
          name = "${pkg.name}-aethalloc-rust-wrapped";
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
    );
}
