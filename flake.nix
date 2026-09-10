{
  description = "Opinionated code formatter for the Spade language";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        # The repo's .rustfmt.toml uses nightly-only options; `cargo fmt`
        # honors RUSTFMT, so the dev shell points it at a nightly rustfmt.
        nightlyRustfmt = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.rustfmt);
        manifest = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package;

        spadefmt = pkgs.rustPlatform.buildRustPackage {
          pname = "spadefmt";
          version = manifest.version;
          src = self;
          cargoLock = {
            lockFile = ./Cargo.lock;
            # The spade-* crates are git dependencies pinned by rev.
            allowBuiltinFetchGit = true;
          };

          meta = {
            description = manifest.description;
            homepage = "https://github.com/korken89/spadefmt";
            license = pkgs.lib.licenses.gpl3Only;
            mainProgram = "spadefmt";
          };
        };
      in
      {
        packages = {
          inherit spadefmt;
          default = spadefmt;
        };

        apps.default = {
          type = "app";
          program = "${spadefmt}/bin/spadefmt";
          meta = spadefmt.meta;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.rust-analyzer
          ];
          # jemalloc (via spade-lang) configures with -Werror at -O0, and
          # glibc warns that _FORTIFY_SOURCE needs optimization.
          hardeningDisable = [ "fortify" ];
          RUSTFMT = "${nightlyRustfmt}/bin/rustfmt";
        };
      }
    ) // {
      overlays.default = final: prev: {
        spadefmt = self.packages.${final.stdenv.hostPlatform.system}.spadefmt;
      };
    };
}
