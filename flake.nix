{
  description = "Portal";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "portal";
          version = "0.5.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [
            pkgs.wayland
            pkgs.libxkbcommon
            pkgs.vulkan-loader
          ];
          postInstall = ''
            install -Dm644 assets/app-icons/portal.desktop $out/share/applications/portal.desktop
            install -Dm644 assets/app-icons/portal.svg $out/share/icons/hicolor/scalable/apps/portal.svg
            install -Dm644 assets/app-icons/png/portal-128.png $out/share/icons/hicolor/128x128/apps/portal.png
            install -Dm644 assets/app-icons/png/portal-256.png $out/share/icons/hicolor/256x256/apps/portal.png
            install -Dm644 assets/app-icons/png/portal-512.png $out/share/icons/hicolor/512x512/apps/portal.png
          '';
        };

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.rustc
            pkgs.cargo
            pkgs.pkg-config
          ];
          buildInputs = [
            pkgs.wayland
            pkgs.libxkbcommon
            pkgs.vulkan-loader
          ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
            pkgs.wayland
            pkgs.libxkbcommon
            pkgs.vulkan-loader
          ];
        };
      });
}
