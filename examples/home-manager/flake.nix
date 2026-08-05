{
  description = "Home-manager config for use inside nix-apptainer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    home-manager = {
      url = "github:nix-community/home-manager/release-26.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, home-manager, ... }:
    let
      # Change to "aarch64-linux" on ARM clusters.
      pkgs = import nixpkgs { system = "x86_64-linux"; };
    in {
      homeConfigurations.container =
        home-manager.lib.homeManagerConfiguration {
          inherit pkgs;
          modules = [
            ({ pkgs, ... }: {
              home.stateVersion = "26.05";
              home.username =
                let u = builtins.getEnv "USER";
                in if u == "" then "nobody" else u;
              home.homeDirectory =
                let h = builtins.getEnv "HOME";
                in if h == "" then "/homeless-shelter" else h;

              programs.git.enable = true;
              programs.direnv.enable = true;
              programs.fzf.enable = true;

              programs.fish = {
                enable = true;
                shellAbbrs = {
                  gs = "git status";
                  ga = "git add";
                  gd = "git diff";
                };
              };
              # chsh can't work in a container; hand interactive
              # sessions from bash to fish with $SHELL set.
              programs.bash = {
                enable = true;
                initExtra = ''
                  if [[ $- == *i* ]]; then
                    exec env SHELL=${pkgs.fish}/bin/fish ${pkgs.fish}/bin/fish -l
                  fi
                '';
              };
            })
          ];
        };
    };
}
