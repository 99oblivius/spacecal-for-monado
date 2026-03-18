{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix.url = "github:nix-community/fenix";
  };

  outputs = {
    self,
    flake-utils,
    naersk,
    nixpkgs,
    fenix,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = (import nixpkgs) {
          inherit system;
          overlays = [fenix.overlays.default];
        };

        lib = pkgs.lib;

        naersk' = pkgs.callPackage naersk {};
      in rec {
        defaultPackage = naersk'.buildPackage {
          src = ./.;
          buildInputs = with pkgs; [
            openxr-loader
            libadwaita
            gtk4
            pkg-config
            monado
            makeWrapper
          ];
          postInstall = ''
            wrapProgram $out/bin/spacecal-for-monado \
              --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [pkgs.openxr-loader]}" \
              --prefix LD_LIBRARY_PATH : "/run/opengl-driver/lib" \
              --run '
                  if [ -z "$XR_RUNTIME_JSON" ]; then
                      ACTIVE_RUNTIME="$HOME/.config/openxr/1/active_runtime.json"
                      if [ -f "$ACTIVE_RUNTIME" ]; then
                          export XR_RUNTIME_JSON="$ACTIVE_RUNTIME"
                      else
                          ACTIVE_RUNTIME="/etc/xdg/openxr/1/active_runtime.json"
                          if [ -f "$ACTIVE_RUNTIME" ]; then
                              export XR_RUNTIME_JSON="$ACTIVE_RUNTIME"
                          else
                              export XR_RUNTIME_JSON="${pkgs.wivrn}/share/openxr/1/openxr_monado.json"
                          fi
                      fi
                  fi
              '

              mkdir -p $out/share/applications
              mkdir -p $out/share/icons

              cp data/dev.oblivius.spacecal-for-monado.svg $out/share/icons/
              cp data/dev.oblivius.spacecal-for-monado.desktop $out/share/applications/
          '';
        };

        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            alejandra
            rust-analyzer
            (pkgs.fenix.stable.withComponents [
              "cargo"
              "clippy"
              "rust-src"
              "rustc"
              "rustfmt"
            ])
          ];
        };
      }
    );
}
