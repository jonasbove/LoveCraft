{
  # Adapted from CraftCloud's flake.nix (MIT OR Apache-2.0), itself modelled on craft-launcher's
  # (MIT OR Apache-2.0, Copyright (c) 2026 ArtCraft Team); see NOTICE.
  #
  #   nix run github:jonasbove/LoveCraft                 # the app
  #   nix run github:jonasbove/LoveCraft#lovecraft-mcp   # the MCP server (PROSECRAFT_OPEN_DIR=…)
  #   nix develop                                        # a shell for cargo and `cargo xtask ci`
  #
  # LoveCraft was ProseCraft-rs until 0.2.0: the package also installs the old commands
  # (prosecraft-rs, prosecraft-mcp) as links, and `packages.prosecraft-rs` and
  # `apps.prosecraft-mcp` remain as aliases.
  description = "LoveCraft: a novel-writing app (outline, write, publish), and its MCP server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # The interface and manuscript fonts (Inter, Source Serif 4) the build embeds, at the commit
    # the release workflow uses (CRAFT_FONTS_REF in .github/workflows/release.yml).
    craft-fonts = {
      url = "github:storytold/craft-fonts/cc9269cd8d8258785dac603fba3122e5d2556795";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, craft-fonts }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rustfmt" "clippy" "rust-src" ];
          targets = [ "wasm32-unknown-unknown" ];
        };
        # Packages build with the minimal toolchain (compiler and std only); the full one above is for
        # the dev shell. The binaries must not keep any toolchain alive: std's source paths in panic
        # messages would otherwise make the ~2 GB toolchain a runtime dependency (see postInstall).
        rustMinimal = pkgs.rust-bin.stable.latest.minimal;
        rustPlatform = pkgs.makeRustPlatform { cargo = rustMinimal; rustc = rustMinimal; };

        version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
        # Keeps the app's former name: desktops know the app by it.
        appId = "io.github.jonasbove.prosecraft_rs";

        # What the window loads at run time (dlopen, so not ELF NEEDED entries): winit's Wayland and
        # X11 libraries and egui's OpenGL through libglvnd, which finds the system's driver (on
        # NixOS, /run/opengl-driver) by itself. The file dialogs use the XDG desktop portal over
        # D-Bus in pure Rust: no GTK or libdbus.
        runtimeLibs = with pkgs; [
          wayland
          libxkbcommon
          libGL
          libx11
          libxcursor
          libxrandr
          libxi
          libxcb
        ];

        lovecraft = rustPlatform.buildRustPackage {
          pname = "lovecraft";
          inherit version;
          # Only what the Rust workspace builds from, and the Linux desktop files.
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./.cargo ./crates ./apps ./xtask ./packaging/linux ];
          };
          cargoLock = {
            lockFile = ./Cargo.lock;
            # Git dependencies (the storytold egui/winit forks, craft-libs) at their locked revisions.
            allowBuiltinFetchGit = true;
          };
          # The cargo packages keep their first names; their binaries are lovecraft and lovecraft-mcp.
          cargoBuildFlags = [ "-p" "prosecraft-rs" "-p" "prosecraft-mcp" ];
          env = {
            CRAFT_FONTS_DIR = "${craft-fonts}";
            CRAFT_FONTS_REQUIRED = "1";
          };
          nativeBuildInputs = [ pkgs.pkg-config pkgs.autoPatchelfHook pkgs.removeReferencesTo ];
          # libgcc_s for the unwinder, as every Rust binary needs.
          buildInputs = runtimeLibs ++ [ pkgs.stdenv.cc.cc.lib ];
          # autoPatchelfHook adds these to the binaries' RUNPATH, so the dlopen calls find them.
          runtimeDependencies = runtimeLibs;
          # The app menu entry, the .prosecraft file type, the AppStream file and icons, all named
          # for the window's app id. Opening links and wiki files uses the desktop's own xdg-open.
          postInstall = ''
            remove-references-to -t ${rustMinimal} $out/bin/lovecraft $out/bin/lovecraft-mcp
            ln -s lovecraft $out/bin/prosecraft-rs
            ln -s lovecraft-mcp $out/bin/prosecraft-mcp
            install -Dm644 packaging/linux/${appId}.desktop $out/share/applications/${appId}.desktop
            install -Dm644 packaging/linux/${appId}.mime.xml $out/share/mime/packages/${appId}.xml
            mkdir -p $out/share/metainfo $out/share/icons
            sed -e "s/@VERSION@/${version}/g" -e "s/@DATE@/1970-01-01/g" \
              packaging/linux/${appId}.metainfo.xml.in > $out/share/metainfo/${appId}.metainfo.xml
            cp -R packaging/linux/icons/hicolor $out/share/icons/
          '';
          disallowedReferences = [ rustMinimal ];
          doCheck = false;
          doInstallCheck = true;
          installCheckPhase = ''
            runHook preInstallCheck
            [ "$($out/bin/lovecraft --version)" = "LoveCraft ${version}" ]
            [ "$($out/bin/prosecraft-rs --version)" = "LoveCraft ${version}" ]
            runHook postInstallCheck
          '';
          meta = {
            description = "Novel-writing app: outline, write and publish (a Rust rewrite of ProseCraft)";
            homepage = "https://github.com/jonasbove/LoveCraft";
            license = with lib.licenses; [ mit asl20 ];
            mainProgram = "lovecraft";
            platforms = [ "x86_64-linux" "aarch64-linux" ];
          };
        };
      in
      {
        packages.default = lovecraft;
        packages.lovecraft = lovecraft;
        packages.prosecraft-rs = lovecraft;

        apps.default = {
          type = "app";
          program = "${lovecraft}/bin/lovecraft";
          meta.description = "LoveCraft, the app";
        };
        apps.lovecraft-mcp = {
          type = "app";
          program = "${lovecraft}/bin/lovecraft-mcp";
          meta.description = "lovecraft-mcp, the MCP server for a ProseCraft open sync folder";
        };
        apps.prosecraft-mcp = {
          type = "app";
          program = "${lovecraft}/bin/lovecraft-mcp";
          meta.description = "lovecraft-mcp under its 0.1 name";
        };

        # cargo, clippy, rustfmt and the wasm target for `cargo xtask ci`, trunk for the web build,
        # and the window's libraries for `cargo run -p prosecraft-rs`.
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ rust pkgs.pkg-config pkgs.trunk ];
          buildInputs = runtimeLibs;
          LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
        };
      });
}
