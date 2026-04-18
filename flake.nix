{
  description = "A simple flake that allows this specific rust project to build for linux and windows. Both inside and outside a shell!";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    # Useful lib for caching cargo builds
    naersk = {
      url = "github:nmattia/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Provide toolchain profiles for rust
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Don't really know
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
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        llvmPackages = pkgs.llvmPackages_21;

        buildToolchain =
          with fenix.packages.${system};
          combine ([
            minimal.rustc
            minimal.cargo
            targets.x86_64-pc-windows-gnu.latest.rust-std
          ]);

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

        naerskLib = naersk.lib.${system}.override {
          cargo = buildToolchain;
          rustc = buildToolchain;
        };

        ciChecks = pkgs.writeShellApplication {
          name = "ci-checks";
          runtimeInputs = [
            pkgs.cargo-deny
          ];
          text = ''
            set -euo pipefail
            cargo fmt -- --check
            cargo-deny check
          '';
        };

        ciTestBuild = pkgs.writeShellApplication {
          name = "ci-test-build";
          text = ''
            set -euo pipefail
            cargo test
            cargo build --release
          '';
        };

        ciLocal = pkgs.writeShellApplication {
          name = "ci-local";
          runtimeInputs = [ ciChecks ciTestBuild ];
          text = ''
            set -euo pipefail
            ci-checks
            ci-test-build
          '';
        };

        unixBuildDeps = with pkgs; [
          sqlite
          gcc
          linuxHeaders
          openssl
          pkg-config
          xorg.libX11
          xorg.libXi
          xorg.libXtst
          wayland
          # We need to be able to cross compile it inside a shell to have LSP capabilities
          pkgsCross.mingwW64.stdenv.cc
        ];

        bindgenClangArgs = pkgs.lib.concatStringsSep " " [
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libc-crt1-cflags")
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libc-cflags")
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/cc-cflags")
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libcxx-cxxflags")
          "-isystem ${pkgs.linuxHeaders}/include"
          (pkgs.lib.optionalString pkgs.stdenv.cc.isClang "-idirafter ${pkgs.stdenv.cc.cc}/lib/clang/${pkgs.lib.getVersion pkgs.stdenv.cc.cc}/include")
          (pkgs.lib.optionalString pkgs.stdenv.cc.isGNU "-isystem ${pkgs.stdenv.cc.cc}/include/c++/${pkgs.lib.getVersion pkgs.stdenv.cc.cc} -isystem ${pkgs.stdenv.cc.cc}/include/c++/${pkgs.lib.getVersion pkgs.stdenv.cc.cc}/${pkgs.stdenv.hostPlatform.config} -idirafter ${pkgs.stdenv.cc.cc}/lib/gcc/${pkgs.stdenv.hostPlatform.config}/${pkgs.lib.getVersion pkgs.stdenv.cc.cc}/include")
        ];
      in
      rec {
        packages = {
          linux = naerskLib.buildPackage {
            src = ./.;
            nativeBuildInputs = unixBuildDeps ++ [
              llvmPackages.libclang
              llvmPackages.clang
            ];
            LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
            BINDGEN_EXTRA_CLANG_ARGS = bindgenClangArgs;
          };

          windows = naerskLib.buildPackage {
            src = ./.;
            doCheck = true;
            singleStep = true;

            nativeBuildInputs = with pkgs; [
              pkgsCross.mingwW64.stdenv.cc
            ];

            buildInputs = with pkgs; [
              pkgsCross.mingwW64.windows.pthreads
              pkgsCross.mingwW64.sqlite
            ];

            # Bunch of compilation flags to make it build successfully
            CARGO_BUILD_TARGET = "x86_64-pc-windows-gnu";
            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/x86_64-w64-mingw32-gcc";
            TARGET_CC = "${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/${pkgs.pkgsCross.mingwW64.stdenv.cc.targetPrefix}cc";
            CARGO_BUILD_RUSTFLAGS = [
              "-C"
              "target-feature=+crt-static"
              # https://github.com/rust-lang/cargo/issues/4133
              "-C"
              "linker=${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/${pkgs.pkgsCross.mingwW64.stdenv.cc.targetPrefix}cc"
            ];
          };
        };

        defaultPackage = packages.linux;

        # Personal Development shell :D
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = unixBuildDeps;
          buildInputs = [
            # Required
            devToolchain
            llvmPackages.libclang
            llvmPackages.clang
            pkgs.cargo-deny
            pkgs.codespell
            pkgs.pkgsCross.mingwW64.sqlite
            pkgs.pkgsCross.mingwW64.windows.pthreads
            ciChecks
            ciTestBuild
            ciLocal
            # Optional
            pkgs.evtest
            pkgs.rust-analyzer
            pkgs.cargo-watch
            pkgs.sqlitebrowser
            pkgs.wine64
            pkgs.crush
          ];

          # Used by bindgen
          LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";

          # From: https://github.com/NixOS/nixpkgs/blob/1fab95f5190d087e66a3502481e34e15d62090aa/pkgs/applications/networking/browsers/firefox/common.nix#L247-L253
          # Set C flags for Rust's bindgen program. Unlike ordinary C
          # compilation, bindgen does not invoke $CC directly. Instead it
          # uses LLVM's libclang. To make sure all necessary flags are
          # included we need to look in a few places. We also import variables that contains other libs used for build here so they
          # are available inside the shell
          shellHook = ''
            export WINEPREFIX=$HOME/.wine64
            export WINEARCH=win64
            [ ! -d "$WINEPREFIX" ] && wineboot
            export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${pkgs.lib.makeLibraryPath unixBuildDeps}";
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/x86_64-w64-mingw32-gcc"
            export BINDGEN_EXTRA_CLANG_ARGS="${bindgenClangArgs}"
          '';
        };
      }
    );
}
