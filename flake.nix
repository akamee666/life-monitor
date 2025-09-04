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
        lib = pkgs.lib;

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

        # Linux deps
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

        windowsLinker = "${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/x86_64-w64-mingw32-gcc";
      in
      rec {
        packages = {
          linux = naersk-lib.buildPackage {
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

        defaultPackage = packages.linux;

        # Development shell :D
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = all_deps;
          buildInputs = [
            devToolchain
            pkgs.evtest
            pkgs.llvmPackages_14.libclang
            pkgs.llvmPackages_14.clang
            pkgs.pkgsCross.mingwW64.sqlite
            pkgs.pkgsCross.mingwW64.windows.pthreads
            pkgs.rust-analyzer
            pkgs.lazygit
            pkgs.cargo-watch
            pkgs.sqlitebrowser
          ];

          LIBCLANG_PATH = "${pkgs.llvmPackages_14.libclang.lib}/lib";

          # From: https://github.com/NixOS/nixpkgs/blob/1fab95f5190d087e66a3502481e34e15d62090aa/pkgs/applications/networking/browsers/firefox/common.nix#L247-L253
          # Set C flags for Rust's bindgen program. Unlike ordinary C
          # compilation, bindgen does not invoke $CC directly. Instead it
          # uses LLVM's libclang. To make sure all necessary flags are
          # included we need to look in a few places.
          shellHook = ''
            export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${pkgs.lib.makeLibraryPath all_deps}";
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${windowsLinker}"
            export BINDGEN_EXTRA_CLANG_ARGS="$(< ${pkgs.stdenv.cc}/nix-support/libc-crt1-cflags) \
              $(< ${pkgs.stdenv.cc}/nix-support/libc-cflags) \
              $(< ${pkgs.stdenv.cc}/nix-support/cc-cflags) \
              $(< ${pkgs.stdenv.cc}/nix-support/libcxx-cxxflags) \
              ${lib.optionalString pkgs.stdenv.cc.isClang "-idirafter ${pkgs.stdenv.cc.cc}/lib/clang/${lib.getVersion pkgs.stdenv.cc.cc}/include"} \
              ${lib.optionalString pkgs.stdenv.cc.isGNU "-isystem ${pkgs.stdenv.cc.cc}/include/c++/${lib.getVersion pkgs.stdenv.cc.cc} -isystem ${pkgs.stdenv.cc.cc}/include/c++/${lib.getVersion pkgs.stdenv.cc.cc}/${pkgs.stdenv.hostPlatform.config} -idirafter ${pkgs.stdenv.cc.cc}/lib/gcc/${pkgs.stdenv.hostPlatform.config}/${lib.getVersion pkgs.stdenv.cc.cc}/include"} \
            "
          '';
        };
      }
    );
}
