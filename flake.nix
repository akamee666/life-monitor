{
  description = "Development and build flake for life-monitor";

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
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        lib = pkgs.lib;
        llvmPackages = pkgs.llvmPackages_21;
        # MinGW cross toolchain used when we intentionally build the Windows target.
        mingw = pkgs.pkgsCross.mingwW64;

        linuxTarget = "x86_64-unknown-linux-gnu";
        windowsTarget = "x86_64-pc-windows-gnu";

        # Target-specific tool paths used by Cargo/cc-rs for Windows cross builds.
        windowsLinker = "${mingw.stdenv.cc}/bin/${mingw.stdenv.cc.targetPrefix}cc";
        windowsAr = "${mingw.stdenv.cc.bintools}/bin/${mingw.stdenv.cc.targetPrefix}ar";
        windowsRanlib = "${mingw.stdenv.cc.bintools}/bin/${mingw.stdenv.cc.targetPrefix}ranlib";
        windowsCxx = "${mingw.stdenv.cc}/bin/${mingw.stdenv.cc.targetPrefix}g++";

        # Small build toolchain used by naersk builds.
        buildToolchain =
          with fenix.packages.${system};
          combine [
            minimal.cargo
            minimal.rustc
            targets.${windowsTarget}.latest.rust-std
          ];

        # Richer toolchain for interactive development.
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

        # bindgen uses libclang directly rather than invoking $CC, so we have to
        # pass the host C toolchain and Linux headers explicitly.
        bindgenClangArgs = lib.concatStringsSep " " [
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libc-crt1-cflags")
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libc-cflags")
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/cc-cflags")
          (builtins.readFile "${pkgs.stdenv.cc}/nix-support/libcxx-cxxflags")
          "-isystem ${pkgs.linuxHeaders}/include"
          (
            lib.optionalString pkgs.stdenv.cc.isClang
              "-idirafter ${pkgs.stdenv.cc.cc}/lib/clang/${lib.getVersion pkgs.stdenv.cc.cc}/include"
          )
          (
            lib.optionalString pkgs.stdenv.cc.isGNU
              "-isystem ${pkgs.stdenv.cc.cc}/include/c++/${lib.getVersion pkgs.stdenv.cc.cc} -isystem ${pkgs.stdenv.cc.cc}/include/c++/${lib.getVersion pkgs.stdenv.cc.cc}/${pkgs.stdenv.hostPlatform.config} -idirafter ${pkgs.stdenv.cc.cc}/lib/gcc/${pkgs.stdenv.hostPlatform.config}/${lib.getVersion pkgs.stdenv.cc.cc}/include"
          )
        ];

        commonBuildEnv = {
          LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = bindgenClangArgs;
        };

        ciChecks = pkgs.writeShellApplication {
          name = "ci-checks";
          runtimeInputs = [ pkgs.cargo-deny ];
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
          runtimeInputs = [ ciChecks ciTestBuild ];
          text = ''
            set -euo pipefail
            ci-checks
            ci-test-build
          '';
        };

        # Native Linux package output built for the host platform.
        mkLinuxPackage = naerskLib.buildPackage (
          {
            src = ./.;
            CARGO_BUILD_TARGET = linuxTarget;
            nativeBuildInputs = linuxBuildDeps ++ [
              llvmPackages.clang
              llvmPackages.libclang
            ];
            buildInputs = linuxRuntimeDeps;
          }
          // commonBuildEnv
        );

        # Cross-compiled Windows package output. This produces the Windows binary
        # from the current host system; it is not a native Windows build job.
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
            # Package builds should emit the artifact reliably; runtime validation
            # for Windows remains a shell/native-CI concern instead of a Nix build
            # phase requirement.
            doCheck = false;
            singleStep = true;
            nativeBuildInputs = [ mingw.stdenv.cc ];
            buildInputs = [ mingw.windows.pthreads ];
          }
          // commonBuildEnv
        );
      in
      {
        formatter = pkgs.nixfmt-rfc-style;

        packages = {
          linux = mkLinuxPackage;
          windows = mkWindowsPackage;
          default = mkLinuxPackage;
        };

        devShells.default = pkgs.mkShell {
          packages = [
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
          ] ++ linuxRuntimeDeps ++ linuxBuildDeps;

          shellHook = ''
            export LIBCLANG_PATH="${llvmPackages.libclang.lib}/lib"
            export BINDGEN_EXTRA_CLANG_ARGS="${bindgenClangArgs}"

            # Keep host Linux builds on the host toolchain so bundled SQLite and
            # other native C dependencies do not accidentally pick up MinGW headers.
            export CC_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=gcc
            export CXX_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=g++
            export AR_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=ar
            export RANLIB_${builtins.replaceStrings ["-"] ["_"] linuxTarget}=ranlib

            # Expose Windows cross-build tools only for the explicit Windows target.
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${windowsLinker}"
            export CC_${builtins.replaceStrings ["-"] ["_"] windowsTarget}="${windowsLinker}"
            export CXX_${builtins.replaceStrings ["-"] ["_"] windowsTarget}="${windowsCxx}"
            export AR_${builtins.replaceStrings ["-"] ["_"] windowsTarget}="${windowsAr}"
            export RANLIB_${builtins.replaceStrings ["-"] ["_"] windowsTarget}="${windowsRanlib}"
            export PKG_CONFIG_ALLOW_CROSS=1

            export WINEPREFIX="$HOME/.wine64"
            export WINEARCH=win64

            cat <<'EOF'
life-monitor dev shell
  host target:    ${linuxTarget}
  windows target: ${windowsTarget}

Common commands:
  cargo test --target ${linuxTarget}
  cargo build --target ${linuxTarget}
  cargo build --target ${windowsTarget}
  cargo check --target ${windowsTarget}
  nix build .#linux
  nix build .#windows
EOF
          '';
        };
      }
    );
}
