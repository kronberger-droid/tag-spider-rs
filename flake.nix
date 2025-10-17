{
  description = "Rust web-crawler and indexer development shell with Fenix";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url  = "github:numtide/flake-utils";
    fenix.url        = "github:nix-community/fenix";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, fenix, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ fenix.overlays.default rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        # Use Fenix's complete stable toolchain and latest rust-analyzer
        stableToolchain = fenix.packages.${system}.complete.toolchain;
        rustAnalyzer    = fenix.packages.${system}.latest.rust-analyzer;

        # Reusable build inputs
        rustInputs = with pkgs; [
          stableToolchain
          rustAnalyzer
          cargo-expand
          rusty-man
        ];

        baseInputs = with pkgs; [
          jq
          pkg-config
          openssl
        ];

        browserInputs = with pkgs; [
          firefox
          geckodriver
        ];

        # Reusable shell hooks
        baseShellHook = ''
          echo "Using Rust toolchain: $(rustc --version)"
          export OPENSSL_DIR=${pkgs.openssl.dev}
          export PKG_CONFIG_PATH=${pkgs.pkg-config}/lib/pkgconfig
          export RUST_BACKTRACE=1
          # Ensure local cargo cache in home dir
          export CARGO_HOME="$HOME/.cargo"
          # Avoid accidental writes to Nix store; RUSTUP_HOME not used by Fenix
          export RUSTUP_HOME="$HOME/.rustup"
          mkdir -p "$CARGO_HOME" "$RUSTUP_HOME"
        '';

        geckoShellHook = ''
          if ! pgrep -x geckodriver > /dev/null; then
            echo "Starting geckodriver..."
            geckodriver > geckodriver.log 2>&1 &
            trap "kill $!" EXIT
          fi
        '';

        nuShellHook = ''
          exec nu --login
        '';
      in {
        devShells = {
          # Default shell with just Rust tools (no geckodriver)
          default = pkgs.mkShell {
            name = "rust-web-crawler-shell";
            buildInputs = lib.flatten [ rustInputs baseInputs ];
            shellHook = baseShellHook + nuShellHook;
          };

          # Shell with geckodriver for browser automation
          gecko = pkgs.mkShell {
            name = "rust-web-crawler-gecko-shell";
            buildInputs = lib.flatten [ rustInputs baseInputs browserInputs ];
            shellHook = baseShellHook + geckoShellHook + nuShellHook;
          };
        };
      }
    );
}
