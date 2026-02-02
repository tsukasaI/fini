{
  description = "A lightweight file normalization CLI tool for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages = {
          fini = pkgs.rustPlatform.buildRustPackage {
            pname = "fini";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            meta = with pkgs.lib; {
              description = "A lightweight file normalization CLI tool for AI coding agents";
              homepage = "https://github.com/tsukasaI/fini";
              license = licenses.mit;
              maintainers = [ ];
              mainProgram = "fini";
            };
          };
          default = self.packages.${system}.fini;
        };

        apps = {
          fini = flake-utils.lib.mkApp {
            drv = self.packages.${system}.fini;
          };
          default = self.apps.${system}.fini;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
          ];
        };
      }
    );
}
