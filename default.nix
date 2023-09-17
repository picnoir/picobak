{ pkgs ? import <nixpkgs> {}, lib ? pkgs.lib }:

pkgs.rustPlatform.buildRustPackage {
  pname = "picobak";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoHash = "sha256-W0SLjlrqONMdTXoOlMilEvza2WEIVaKUJRraGR//qsw=";
  meta = {
    description = "Backup and organize your pictures library";
  };
  nativeBuildInputs = [ pkgs.makeWrapper ];
  # Inject exiftool
  postInstall = ''
    wrapProgram $out/bin/picobak \
      --prefix PATH : "${lib.makeBinPath [pkgs.exiftool]}"
  '';
}
