{
  description = "Bioinformatics dev environment — R, Python, samtools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs =
    { nixpkgs, ... }:
    let
      # Change to "aarch64-linux" on ARM clusters.
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      rEnv = pkgs.rWrapper.override {
        packages = with pkgs.rPackages; [
          dplyr
          tidyr
          ggplot2
          DESeq2
        ];
      };

      pythonEnv = pkgs.python3.withPackages (
        ps: with ps; [
          numpy
          pandas
        ]
      );

    in
    {
      devShells.${system}.default = pkgs.mkShellNoCC {
        packages = [
          rEnv
          pythonEnv
          pkgs.samtools
        ];
      };
    };
}
