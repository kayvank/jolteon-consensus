{
  description = "A nix flake for jolteon consensus rust impl";
  inputs = {
    nixpkgs.url = "github:NixOs/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";
  };
  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      pre-commit-hooks,
      ...
    }@inputs:

    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
          ];
        };
        hook = pre-commit-hooks.lib.${system};
        tools = import "${pre-commit-hooks}/nix/call-tools.nix" pkgs;
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
        cargoTomlContents = builtins.readFile ../Cargo.toml;
        version = (builtins.fromTOML cargoTomlContents).package.version;

        btc = pkgs.rustPlatform.buildRustPackage {
          inherit version;
          name = "btc";
          buildInputs = with pkgs; [ openssl ];
          nativeBuildInputs = with pkgs; [
            pkg-config
            openssl.dev
          ];

          src = pkgs.lib.cleanSourceWith { src = self; };

          cargoLock.lockFile = ../Cargo.lock;

        };
      in
      rec {
        checks.pre-commit-check = hook.run {
          src = self;
          tools = tools;
          # enforce pre-commit-hook
          hooks = {
            rustfmt.enable = true;
            nixfmt-rfc-style.enable = true;
          };
        };

        overlays.default = final: prev: { btc = btc; };

        gitRev = if (builtins.hasAttr "rev" self) then self.rev else "dirty";

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            nerdfetch
            openssl
            pkg-config
            protobuf
            curl
            cmake
            ninja
            # Rust stuff (dev only)
            eza
            rust-analyzer-unwrapped
            watchexec
            # Rust stuff (CI + dev)
            toolchain
            cargo-deny
            # Spelling and linting
            codespell
          ];
          packages = with pkgs; [
            tools.nixpkgs-fmt
            tools.nixfmt-rfc-style
          ];

          shellHook = ''
            ${checks.pre-commit-check.shellHook}
            export RUST_SRC_PATH="${toolchain}/lib/rustlib/src/rust/library"
            export CARGO_HOME="$(pwd)/.cargo"
            export PATH="$CARGO_HOME/bin:$PATH"
            #
            # Application specific
            #
            ##export RUST_BACKTRACE=1
            ##export RUST_LOG='debug'
            nerdfetch
          '';
        };
      }
    );
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
