{
  description = "gh-read development and build environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;

      rustVersion = "1.96.0";
      mkRustToolchain =
        pkgs:
        pkgs.rust-bin.stable.${rustVersion}.default.override {
          extensions = [
            "clippy"
            "rust-analyzer"
            "rust-src"
            "rustfmt"
          ];
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

          rustToolchain = mkRustToolchain pkgs;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = "gh-read";
            version = "0.3.0";

            src = self;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [ pkgs.makeWrapper ];
            nativeCheckInputs = [
              pkgs.jq
            ];
            postInstall = ''
              wrapProgram "$out/bin/gh-read" --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.gh ]}"
            '';

            meta = {
              description = "Read fixed GitHub pull request and issue metadata through the GitHub CLI";
              homepage = "https://github.com/riii111/gh-read";
              license = pkgs.lib.licenses.mit;
              mainProgram = "gh-read";
            };
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = [
              (mkRustToolchain pkgs)
              pkgs.cargo-audit
              pkgs.cargo-machete
              pkgs.cargo-nextest
              pkgs.gh
              pkgs.jq
              pkgs.lefthook
            ];
          };
        }
      );

      formatter = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        pkgs.nixfmt
      );
    };
}
