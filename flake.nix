{
  inputs = {
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { self, fenix, nixpkgs }:
    let
      # Define the system you're building for once
      system = "x86_64-linux";
      # Get the packages for the system
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ fenix.overlays.default ];
      };

      all_deps = with pkgs; [
        sqlite
        gcc
        openssl 
        pkg-config
        xorg.libX11
        xorg.libXi
        xorg.libXtst
      ];
    in
    {
      packages.${system}.default = fenix.packages.${system}.minimal.toolchain;
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = all_deps; 
        buildInputs = [

          # Add the required components from fenix
          (fenix.packages.${system}.complete.withComponents [
            "cargo"
            "clippy"
            "rust-src"
            "rustc"
            "rustfmt"
          ])
        ];
         shellHook = ''
                export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${pkgs.lib.makeLibraryPath all_deps}";
         '';
      };
    };
}
