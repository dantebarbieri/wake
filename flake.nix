{
  description = "wake: Wake-on-LAN for the machines in the house";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: rec {
        wake = pkgs.rustPlatform.buildRustPackage {
          pname = "wake";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          meta = {
            description = "Wake the machines in the house with Wake-on-LAN";
            license = pkgs.lib.licenses.mit;
            mainProgram = "wake";
          };
        };
        default = wake;
      });
    };
}
