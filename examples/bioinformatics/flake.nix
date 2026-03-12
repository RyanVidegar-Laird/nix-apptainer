{
  description = "Bioinformatics dev environment — R, Python, samtools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
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
      in
      {
        devShells = {
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

          full = pkgs.mkShell {
            packages = [
              rEnv
              pythonEnv
              pkgs.samtools
            ];
            shellHook = ''
              echo "Full bioinformatics environment loaded: R, Python, samtools"
            '';
          };

          default = pkgs.mkShell {
            packages = [
              rEnv
              pythonEnv
              pkgs.samtools
            ];
            shellHook = ''
              echo "Full bioinformatics environment loaded: R, Python, samtools"
            '';
          };
        };
      }
    );
}
