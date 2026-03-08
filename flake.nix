{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{ flake-parts, nixpkgs, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];

      perSystem =
        { pkgs, system, ... }:
        let
          version = "0.1.0";

          nixos = nixpkgs.lib.nixosSystem {
            inherit system;
            modules = [ ./nixos/configuration.nix ];
          };

          buildSandbox = pkgs.callPackage ./lib/build-sandbox.nix { };
          sandbox = buildSandbox { nixosConfig = nixos; };

          buildSif = pkgs.callPackage ./lib/build-sif.nix { };
          sifImage = buildSif { inherit sandbox; };

          scripts = ./scripts;

          cli = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
            pname = "nix-apptainer";
            inherit version;
            src = ./cli;
            cargoLock.lockFile = ./cli/Cargo.lock;
          };
        in
        {
          packages = {
            inherit sandbox cli;
            default = sifImage;
            vm-test = import ./tests/vm-test.nix {
              inherit pkgs sifImage scripts;
            };
          };

          checks = {
            inherit cli;

            eval = import ./tests/eval.nix {
              inherit (pkgs) runCommand;
              nixosConfig = nixos;
            };

            shellcheck = import ./tests/shellcheck.nix {
              inherit (pkgs) runCommand shellcheck;
              inherit scripts;
            };

            sandbox-structure = import ./tests/sandbox-structure.nix {
              inherit (pkgs) runCommand jq;
              inherit sandbox;
            };

            sif-contents = import ./tests/sif-contents.nix {
              inherit (pkgs) runCommand squashfsTools apptainer;
              inherit sifImage;
            };
          };

          devShells.default = pkgs.mkShell {
            packages = with pkgs; [
              apptainer
              squashfsTools
              fuse-overlayfs
              nixfmt-rfc-style
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
            ];
          };

          formatter = pkgs.nixfmt-rfc-style;
        };
    };
}
