{ pkgs ? import <nixpkgs> {}, lib ? pkgs.lib }:

pkgs.rustPlatform.buildRustPackage {
  pname = "picobak";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoHash = "sha256-gytrsYdL9WuxJDZBaK/w+1KLmAKKBD711efHTzQqs4o=";
  meta = {
    description = "Backup and organize your pictures library";
  };
}
