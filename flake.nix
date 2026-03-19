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

        benchmarkSources = [
          "packet_churn"
          "tail_latency"
          "producer_consumer"
          "fragmentation"
          "multithread_churn"
        ];

        buildBenchmark = name: pkgs.stdenv.mkDerivation {
          pname = "bench-${name}";
          version = "0.1.0";
          src = ./benches;
          
          buildPhase = ''
            gcc -O3 -pthread -o ${name} ${name}.c
          '';
          
          installPhase = ''
            mkdir -p $out/bin
            cp ${name} $out/bin/
          '';
        };

        benchmarks = pkgs.symlinkJoin {
          name = "aethalloc-benchmarks";
          paths = map buildBenchmark benchmarkSources;
        };

        allocators = {
          aethalloc = {
            name = "aethalloc";
            libPath = "${aethalloc-rs}/lib/libaethalloc.so";
            description = "AethAlloc - High-performance allocator for network workloads";
          };
          mimalloc = {
            name = "mimalloc";
            libPath = "${pkgs.mimalloc}/lib/libmimalloc.so";
            description = "Microsoft mimalloc - General purpose allocator with excellent performance";
          };
          jemalloc = {
            name = "jemalloc";
            libPath = "${pkgs.jemalloc}/lib/libjemalloc.so.2";
            description = "jemalloc - General purpose malloc emphasizing fragmentation avoidance";
          };
          tcmalloc = {
            name = "tcmalloc";
            libPath = "${pkgs.gperftools}/lib/libtcmalloc.so";
            description = "Google tcmalloc - Thread-caching malloc";
          };
          glibc = {
            name = "glibc";
            libPath = "";
            description = "glibc ptmalloc2 - Default Linux allocator";
          };
        };

        runBenchmarks = pkgs.writeShellScriptBin "run-alloc-benchmarks" ''
          set -euo pipefail
          
          BENCH_DIR="${benchmarks}/bin"
          RESULTS_DIR="''${1:-./benchmark-results}"
          TIMESTAMP=$(date +%Y%m%d_%H%M%S)
          RUN_DIR="$RESULTS_DIR/$TIMESTAMP"
          
          mkdir -p "$RUN_DIR"
          
          ITERATIONS=''${ITERATIONS:-100000}
          THREADS=''${THREADS:-8}
          WARMUP=''${WARMUP:-10000}
          
          echo "Running allocator benchmarks..."
          echo "Results will be stored in: $RUN_DIR"
          echo "Iterations: $ITERATIONS, Threads: $THREADS"
          echo ""
          
          get_lib_path() {
            case "$1" in
              aethalloc) echo "${aethalloc-rs}/lib/libaethalloc.so" ;;
              mimalloc)  echo "${pkgs.mimalloc}/lib/libmimalloc.so" ;;
              jemalloc)  echo "${pkgs.jemalloc}/lib/libjemalloc.so.2" ;;
              tcmalloc)  echo "${pkgs.gperftools}/lib/libtcmalloc.so" ;;
              glibc)     echo "" ;;
            esac
          }
          
          get_benchmark_args() {
            case "$1" in
              packet_churn)      echo "$ITERATIONS $WARMUP" ;;
              tail_latency)      echo "$THREADS $ITERATIONS" ;;
              producer_consumer) echo "4 4" ;;
              fragmentation)     echo "$ITERATIONS 100000" ;;
              multithread_churn) echo "$THREADS $ITERATIONS" ;;
            esac
          }
          
          run_benchmark() {
            local bench="$1"
            local alloc="$2"
            local lib_path="$3"
            local args="$4"
            local output_file="$RUN_DIR/''${bench}_''${alloc}.json"
            
            echo -n "Running $bench with $alloc... "
            
            if [ -n "$lib_path" ]; then
              LD_PRELOAD="$lib_path" "$BENCH_DIR/$bench" $args > "$output_file" 2>&1
            else
              "$BENCH_DIR/$bench" $args > "$output_file" 2>&1
            fi
            
            echo "done"
          }
          
          ALLOCATORS="aethalloc mimalloc jemalloc tcmalloc glibc"
          BENCHMARKS="packet_churn tail_latency producer_consumer fragmentation multithread_churn"
          
          for bench in $BENCHMARKS; do
            echo ""
            echo "=== Benchmark: $bench ==="
            args=$(get_benchmark_args "$bench")
            for alloc in $ALLOCATORS; do
              lib_path=$(get_lib_path "$alloc")
              run_benchmark "$bench" "$alloc" "$lib_path" "$args"
            done
          done
          
          echo ""
          echo "Benchmark results saved to: $RUN_DIR"
          echo ""
          echo "Summary:"
          for f in "$RUN_DIR"/*.json; do
            echo "  $(basename $f):"
            cat "$f" | head -1
          done
        '';

      in {
        packages = {
          default = aethalloc-rs;
          suricata-aeth = withAethAlloc pkgs.suricata;
          benchmarks = benchmarks;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-flamegraph
            pkgs.valgrind
            pkgs.jq
            benchmarks
            runBenchmarks
            pkgs.mimalloc
            pkgs.jemalloc
            pkgs.gperftools
            pkgs.flamegraph
            pkgs.valgrind
          ];

          shellHook = ''
            export AETHALLOC_LIB="${aethalloc-rs}/lib/libaethalloc.so"
            export MIMALLOC_LIB="${pkgs.mimalloc}/lib/libmimalloc.so"
            export JEMALLOC_LIB="${pkgs.jemalloc}/lib/libjemalloc.so.2"
            export TCMALLOC_LIB="${pkgs.gperftools}/lib/libtcmalloc.so"
            echo "Allocator libraries available:"
            echo "  AethAlloc: $AETHALLOC_LIB"
            echo "  mimalloc:  $MIMALLOC_LIB"
            echo "  jemalloc:  $JEMALLOC_LIB"
            echo "  tcmalloc:  $TCMALLOC_LIB"
            echo ""
            echo "Run benchmarks with: run-alloc-benchmarks [results_dir]"
            echo "For full comparison: FULL_COMPARISON=1 run-alloc-benchmarks"
            echo ""
            echo "Note: snmalloc not available in nixpkgs. Install manually if needed:"
            echo "  https://github.com/microsoft/snmalloc"
          '';
        };
      }
    ) // {
      nixosModules.default = nixosModule;
    };
}
