{
  description = "RStudio Server dev environment for use inside nix-apptainer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs =
    { nixpkgs, ... }:
    let
      # Change to "aarch64-linux" on ARM clusters.
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      # rserver/rsession wrapped so the R environment below is what
      # sessions see. Add R packages here.
      rstudio = pkgs.rstudioServerWrapper.override {
        packages = with pkgs.rPackages; [
          dplyr
          tidyr
          ggplot2
        ];
      };

      # Auth helper (rocker pattern): rserver invokes it with the
      # username as argv[1] and the submitted password on stdin;
      # exit 0 accepts the login.
      pamHelper = pkgs.writeShellScript "rstudio-pam-helper" ''
        IFS= read -r password
        [ -n "''${RSTUDIO_PASSWORD:-}" ] && [ "$password" = "$RSTUDIO_PASSWORD" ]
      '';

      rstudioStart = pkgs.writeShellScriptBin "rstudio-start" ''
        set -euo pipefail

        state_dir="''${XDG_DATA_HOME:-$HOME/.local/share}/rstudio-server"
        mkdir -p "$state_dir"

        # rserver refuses to start without a database it can write.
        printf 'provider=sqlite\ndirectory=%s/db\n' "$state_dir" \
          > "$state_dir/database.conf"

        # A successful connect means the port is taken.
        port_in_use() {
          (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
        }

        # RSTUDIO_PORT unset or "auto": scan from 8787 upward.
        # Numeric: use exactly that port, and fail if busy — an
        # explicit port usually means a tunnel is already set up
        # for it, so silently moving would break it.
        choice="''${RSTUDIO_PORT:-auto}"
        if [ "$choice" = auto ]; then
          port=""
          for p in $(seq 8787 8887); do
            if ! port_in_use "$p"; then port="$p"; break; fi
          done
          if [ -z "$port" ]; then
            echo "rstudio-start: no free port in 8787-8887" >&2
            exit 1
          fi
        else
          port="$choice"
          if port_in_use "$port"; then
            echo "rstudio-start: port $port is in use (unset RSTUDIO_PORT to scan)" >&2
            exit 1
          fi
        fi

        # head reads exactly 30 bytes and exits 0 — no SIGPIPE issues
        # under pipefail (never put head downstream of tr/urandom).
        if [ -z "''${RSTUDIO_PASSWORD:-}" ]; then
          RSTUDIO_PASSWORD="$(head -c 30 /dev/urandom | base64 | tr -d '+/=' | cut -c1-20)"
        fi
        export RSTUDIO_PASSWORD

        echo "RStudio Server starting"
        echo "  user:     $(id -un)"
        echo "  password: $RSTUDIO_PASSWORD"
        echo "  port:     $port"
        echo ""
        echo "On your laptop:"
        echo "  ssh -L 8787:localhost:$port $(id -un)@$(uname -n)"
        echo "then open http://localhost:8787 and log in with the values above."
        echo ""

        exec rserver \
          --server-user="$(id -un)" \
          --www-address=127.0.0.1 \
          --www-port="$port" \
          --server-daemonize=0 \
          --server-data-dir="$state_dir/data" \
          --server-pid-file="$state_dir/rstudio.pid" \
          --secure-cookie-key-file="$state_dir/secure-cookie-key" \
          --database-config-file="$state_dir/database.conf" \
          --auth-none=0 \
          --auth-pam-helper-path=${pamHelper} \
          --auth-timeout-minutes=0 \
          --auth-stay-signed-in-days=7
      '';
    in
    {
      devShells.${system}.default = pkgs.mkShellNoCC {
        packages = [
          rstudio
          rstudioStart
        ];
      };
    };
}
