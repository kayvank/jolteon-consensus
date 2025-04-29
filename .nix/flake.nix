{
  description = "A Rust key-value store";

  inputs = {
    nixpkgs.url = "github:NixOs/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
      in with pkgs; {

        devShells.default = mkShell {
          buildInputs = [
            figlet
            toolchain
            openssl
            pkg-config
            eza
            rust-bin.beta.latest.default
            rust-analyzer-unwrapped
            watchexec
            rustup
          ];
          shellHook = ''
            alias ls='eza --icons'
            alias find=fd

            set -o vi

            export RUST_SRC_PATH="${toolchain}/lib/rustlib/src/rust/library"
            ## Don't pollute local cache of cargo registry index
            ## If you dont care about that, remvoe the line below
            ## to amortize on your existing local cache
            export CARGO_HOME=".cargo"
            export PATH="$CARGO_HOME/bin:$PATH"
            export RUST_BACKTRACE=1
            figlet "rust jolteon consensus"
          '';
        };
      });

  ## use iog cache
  nixConfig = {
    extra-substituters = [
      "https://cache.iog.io"
      "https://cache.sc.iog.io"
    ];
    extra-trusted-public-keys = [
      "hydra.iohk.io:f/Ea+s+dFdN+3Y/G+FDgSq+a5NEWhJGzdjvKNGv0/EQ="
      "cache.sc.iog.io:b4YIcBabCEVKrLQgGW8Fylz4W8IvvfzRc+hy0idqrWU="
    ];
    allow-import-from-derivation = true;
    accept-flake-config = true;
  };

}
