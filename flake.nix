{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    naersk = {
      url = "github:nmattia/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      naersk,
      fenix,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # Toolchain for hermetic naersk builds (unchanged)
        naerskToolchain =
          with fenix.packages.${system};
          combine ([
            minimal.rustc
            minimal.cargo
            targets.x86_64-pc-windows-gnu.latest.rust-std
          ]);

        naersk-lib = naersk.lib.${system}.override {
          cargo = naerskToolchain;
          rustc = naerskToolchain;
        };

        all_deps = with pkgs; [
          sqlite
          gcc
          openssl
          pkg-config
          xorg.libX11
          xorg.libXi
          xorg.libXtst
          pkgsCross.mingwW64.stdenv.cc
        ];

        # NEW: Define a dedicated toolchain for the dev shell
        devToolchain =
          with fenix.packages.${system};
          combine [
            (complete.withComponents [
              "cargo"
              "clippy"
              "rust-src"
              "rustc"
              "rustfmt"
            ])
            # Add the standard library for our windows target
            targets.x86_64-pc-windows-gnu.latest.rust-std
          ];

        # NEW: Define the linker path for Windows cross-compilation
        windowsLinker = "${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/x86_64-w64-mingw32-gcc";

      in
      rec {
        packages = {
          default = naersk-lib.buildPackage {
            src = ./.;
            nativeBuildInputs = all_deps;
          };

          windows = naersk-lib.buildPackage {
            src = ./.;
            nativeBuildInputs = with pkgs; [
              pkgsCross.mingwW64.stdenv.cc
              wineWowPackages.stable
            ];
            buildInputs = with pkgs.pkgsCross.mingwW64; [
              windows.pthreads
              sqlite
            ];
            CARGO_BUILD_TARGET = "x86_64-pc-windows-gnu";
            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER = pkgs.writeScript "wine-wrapper" ''
              export WINEPREFIX="$(mktemp -d)"
              exec wine64 $@
            '';
            doCheck = true;
            singleStep = true;
          };
        };

        defaultPackage = packages.default;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = all_deps;

          buildInputs = [
            devToolchain
            pkgs.pkgsCross.mingwW64.sqlite
            pkgs.pkgsCross.mingwW64.windows.pthreads
            pkgs.rust-analyzer
            pkgs.lazygit
            pkgs.cargo-watch
          ];

          shellHook = ''
            export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${pkgs.lib.makeLibraryPath all_deps}";
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${windowsLinker}"
          '';
        };
      }
    );
}
