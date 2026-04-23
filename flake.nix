{
  description = "Development and build flake for Vigil";

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

  outputs = {
    self,
    nixpkgs,
    naersk,
    fenix,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        lib = pkgs.lib;
        llvmPackages = pkgs.llvmPackages_21;
        mingw = pkgs.pkgsCross.mingwW64;

        linuxTarget = "x86_64-unknown-linux-gnu";
        windowsTarget = "x86_64-pc-windows-gnu";

        windowsLinker = "${mingw.stdenv.cc}/bin/${mingw.stdenv.cc.targetPrefix}cc";
        windowsAr = "${mingw.stdenv.cc.bintools}/bin/${mingw.stdenv.cc.targetPrefix}ar";
        windowsRanlib = "${mingw.stdenv.cc.bintools}/bin/${mingw.stdenv.cc.targetPrefix}
  ranlib";
        windowsCxx = "${mingw.stdenv.cc}/bin/${mingw.stdenv.cc.targetPrefix}g++";

        buildToolchain = with fenix.packages.${system};
          combine [
            minimal.cargo
            minimal.rustc
            targets.${windowsTarget}.latest.rust-std
          ];

        devToolchain = with fenix.packages.${system};
          combine [
            (complete.withComponents [
              "cargo"
              "clippy"
              "rust-src"
              "rustc"
              "rustfmt"
            ])

            targets.${windowsTarget}.latest.rust-std
          ];

        naerskLib = naersk.lib.${system}.override {
          cargo = buildToolchain;
          rustc = buildToolchain;
        };

        linuxRuntimeDeps = with pkgs; [
          openssl
          wayland
          xorg.libX11
          xorg.libXi
          xorg.libXtst
        ];

        linuxBuildDeps = with pkgs; [
          gcc
          linuxHeaders
          pkg-config
        ];

        bindgenClangArgs =
          lib.concatStringsSep " " [
            (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libc-crt1-cflags")
            (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libc-cflags")
            (builtins.readFile "${pkgs.stdenv.cc}/nix-support/cc-cflags")
            (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libcxx-cxxflags")
            "-isystem ${pkgs.linuxHeaders}/include"
            (
              lib.optionalString pkgs.stdenv.cc.isClang
              "-idirafter ${pkgs.stdenv.cc.cc}/lib/clang/${lib.getVersion
                pkgs.stdenv.cc.cc}/include"
            )
            (
              lib.optionalString pkgs.stdenv.cc.isGNU "-isystem ${pkgs.stdenv.cc.cc}/include/c++/${lib.getVersion
                pkgs.stdenv.cc.cc} -isystem ${pkgs.stdenv.cc.cc}/include/c++/${lib.getVersion
                pkgs.stdenv.cc.cc}/${pkgs.stdenv.hostPlatform.config} -idirafter ${pkgs.stdenv.cc.cc}/
  lib/gcc/${pkgs.stdenv.hostPlatform.config}/${lib.getVersion pkgs.stdenv.cc.cc}/
  include"
            )
          ];

        commonBuildEnv = {
          LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = bindgenClangArgs;
        };

        ciChecks = pkgs.writeShellApplication {
          name = "ci-checks";
          runtimeInputs = [pkgs.cargo-deny];
          text = ''
            set -euo pipefail
            cargo fmt --all -- --check
            cargo-deny check
          '';
        };

        ciTestBuild = pkgs.writeShellApplication {
          name = "ci-test-build";
          text = ''
            set -euo pipefail
            cargo test --target ${linuxTarget}
            cargo build --release --target ${linuxTarget}
            cargo check --target ${windowsTarget}
          '';
        };

        ciLocal = pkgs.writeShellApplication {
          name = "ci-local";
          runtimeInputs = [ciChecks ciTestBuild];
          text = ''
            set -euo pipefail
            ci-checks
            ci-test-build
          '';
        };

        mkLinuxPackage = naerskLib.buildPackage (
          {
            src = ./.;
            CARGO_BUILD_TARGET = linuxTarget;
            nativeBuildInputs =
              linuxBuildDeps
              ++ [
                llvmPackages.clang
                llvmPackages.libclang
              ];
            buildInputs = linuxRuntimeDeps;
          }
          // commonBuildEnv
        );

        mkWindowsPackage = naerskLib.buildPackage (
          {
            src = ./.;
            CARGO_BUILD_TARGET = windowsTarget;
            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = windowsLinker;
            CC_x86_64_pc_windows_gnu = windowsLinker;
            CXX_x86_64_pc_windows_gnu = windowsCxx;
            AR_x86_64_pc_windows_gnu = windowsAr;
            RANLIB_x86_64_pc_windows_gnu = windowsRanlib;
            PKG_CONFIG_ALLOW_CROSS = 1;
            CARGO_BUILD_RUSTFLAGS = [
              "-C"
              "target-feature=+crt-static"
              "-C"
              "linker=${windowsLinker}"
            ];
            doCheck = false;
            singleStep = true;
            nativeBuildInputs = [mingw.stdenv.cc];
            buildInputs = [mingw.windows.pthreads];
          }
          // commonBuildEnv
        );

        linuxPackage = mkLinuxPackage;
        windowsPackage = mkWindowsPackage;
      in {
        formatter = pkgs.nixfmt-rfc-style;

        packages = {
          vigil = linuxPackage;
          windows = windowsPackage;
          default = linuxPackage;
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/vigil";
        };

        checks = {
          inherit ciChecks ciTestBuild ciLocal;
          vigil = linuxPackage;
          windows = windowsPackage;
        };

        devShells.default = pkgs.mkShell {
          packages =
            [
              devToolchain
              llvmPackages.clang
              llvmPackages.libclang
              mingw.stdenv.cc
              pkgs.cargo-deny
              pkgs.codespell
              pkgs.rust-analyzer
              pkgs.cargo-watch
              pkgs.sqlitebrowser
              pkgs.evtest
              pkgs.wine64
              pkgs.crush
              ciChecks
              ciTestBuild
              ciLocal
            ]
            ++ linuxRuntimeDeps ++ linuxBuildDeps;

          shellHook = ''
                        export LIBCLANG_PATH="${llvmPackages.libclang.lib}/lib"
                        export BINDGEN_EXTRA_CLANG_ARGS="${bindgenClangArgs}"

                        export CC_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=gcc
                        export CXX_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=g++
                        export AR_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=ar
                        export RANLIB_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=ranlib

                        export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${windowsLinker}"
                        export CC_${builtins.replaceStrings ["-"] ["_"] windowsTarget}
            ="${windowsLinker}"
                        export CXX_${builtins.replaceStrings ["-"] ["_"] windowsTarget}
            ="${windowsCxx}"
                        export AR_${builtins.replaceStrings ["-"] ["_"] windowsTarget}
            ="${windowsAr}"
                        export RANLIB_${builtins.replaceStrings ["-"] ["_"] windowsTarget}
            ="${windowsRanlib}"
                        export PKG_CONFIG_ALLOW_CROSS=1

                        export WINEPREFIX="$HOME/.wine64"
                        export WINEARCH=win64
          '';
        };
      }
    );
}
