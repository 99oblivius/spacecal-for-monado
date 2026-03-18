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
      in {
        packages.default = naersk'.buildPackage {
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
                  for f in \
                    "$HOME/.config/openxr/1/active_runtime.json" \
                    "/etc/xdg/openxr/1/active_runtime.json" \
                    "${pkgs.monado}/share/openxr/1/openxr_monado.json"; do
                    if [ -f "$f" ]; then export XR_RUNTIME_JSON="$f"; break; fi
                  done
                fi
              '

            install -Dm644 data/dev.oblivius.spacecal-for-monado.desktop $out/share/applications/dev.oblivius.spacecal-for-monado.desktop
            install -Dm644 data/dev.oblivius.spacecal-for-monado.svg $out/share/icons/hicolor/scalable/apps/dev.oblivius.spacecal-for-monado.svg
            install -Dm644 data/dev.oblivius.spacecal-for-monado.metainfo.xml $out/share/metainfo/dev.oblivius.spacecal-for-monado.metainfo.xml
          '';
        };

        devShells.default = pkgs.mkShell {
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
            openxr-loader
            libadwaita
            gtk4
            pkg-config
            monado
          ];
        };
      }
    );
}
