{
  description = "Terminal New MY — Material You command interfaces";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      packageFor =
        pkgs:
        pkgs.rustPlatform.buildRustPackage {
          pname = "terminal-new-my";
          version = "0.2.0";
          src = nixpkgs.lib.fileset.toSource {
            root = ./.;
            fileset = nixpkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
              ./tests
              ./tools
              ./integrations
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          nativeCheckInputs = [
            pkgs.python3
            pkgs.gitMinimal
            pkgs.fish
            pkgs.eza
            pkgs.bat
            pkgs.fzf
            pkgs.delta
          ];
          preCheck = ''
            export TNMY_TEST_SH=${pkgs.runtimeShell}
          '';
          postCheck = ''
            python3 tools/pty-check.py target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/tnmy
            python3 tools/integration-check.py target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/tnmy
          '';
          postInstall = ''
            wrapProgram $out/bin/tnmy --prefix PATH : ${
              nixpkgs.lib.makeBinPath [
                pkgs.gitMinimal
                pkgs.bat
              ]
            }
            install -Dm644 integrations/fish/tnmy.fish $out/share/terminal-new-my/tnmy.fish
            install -Dm644 integrations/fish/completions/tnmy.fish $out/share/fish/vendor_completions.d/tnmy.fish
          '';
          meta = {
            description = "Material You file lists and interactive command interfaces";
            homepage = "https://github.com/Kostr2007/Terminal-new-MY";
            license = nixpkgs.lib.licenses.mit;
            mainProgram = "tnmy";
            platforms = systems;
          };
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = packageFor pkgs;
          tnmy = self.packages.${system}.default;
        }
      );
      overlays.default = final: _prev: { terminal-new-my = packageFor final; };
      homeManagerModules.default = import ./nix/home-manager.nix self;
      checks = forAllSystems (system: {
        package = self.packages.${system}.default;
        fish =
          let
            pkgs = nixpkgs.legacyPackages.${system};
          in
          pkgs.runCommand "tnmy-fish-check" { nativeBuildInputs = [ pkgs.fish ]; } ''
            fish --no-config --no-execute ${./integrations/fish/tnmy.fish}
            fish --no-config --no-execute ${./integrations/fish/completions/tnmy.fish}
            touch $out
          '';
      });
      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              fish
              pkg-config
              python3
              git
              bat
              eza
              fzf
              delta
            ];
          };
        }
      );
      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
