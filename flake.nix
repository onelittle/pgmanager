{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        devShell = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
          ];
        };

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "pgmanager";
          version = "0.3.1";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = {
            description = "CLI to manage PostgreSQL databases in test environments";
            homepage = "https://github.com/onelittle/pgmanager";
            license = pkgs.lib.licenses.mit;
            maintainers = ["theodorton"];
            mainProgram = "pgmanager";
          };
        };
      }
    );
}
