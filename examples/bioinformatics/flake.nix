{
  description = "Bioinformatics dev environment — R, Python, samtools";

  inputs = {
    # Must match the container's nixpkgs release (check inside the
    # container: nix eval --raw nixpkgs#lib.trivial.release).
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs = { nixpkgs, ... }:
    let
      # Change to "aarch64-linux" on ARM clusters.
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      rEnv = pkgs.rWrapper.override {
        packages = with pkgs.rPackages; [
          dplyr
          tidyr
          ggplot2
        ];
      };

      pythonEnv = pkgs.python3.withPackages (ps: with ps; [
        numpy
        pandas
      ]);

      fullShell = pkgs.mkShell {
        packages = [
          rEnv
          pythonEnv
          pkgs.samtools
        ];
        shellHook = ''
          echo "Full bioinformatics environment loaded: R, Python, samtools"
        '';
      };
    in
    {
      devShells.${system} = {
        r = pkgs.mkShell {
          packages = [ rEnv ];
          shellHook = ''
            echo "R environment loaded: dplyr, tidyr, ggplot2"
          '';
        };

        python = pkgs.mkShell {
          packages = [ pythonEnv ];
          shellHook = ''
            echo "Python environment loaded: numpy, pandas"
          '';
        };

        samtools = pkgs.mkShell {
          packages = [ pkgs.samtools ];
          shellHook = ''
            echo "samtools $(samtools --version | head -1) loaded"
          '';
        };

        full = fullShell;
        default = fullShell;
      };
    };
}
