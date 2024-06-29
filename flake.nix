{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    fenix.url = "github:nix-community/fenix";
  };

  outputs = { self, nixpkgs, utils, fenix }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        rust-toolchain = with fenix.packages.${system};
          combine (with complete; [
            rustc
            rust-src
            cargo
            clippy
            rustfmt
            rust-analyzer
          ]);
      in
      {
        devShell = with pkgs; mkShell rec {
          buildInputs = [
            alsa-lib
            cargo-bloat
            cargo-criterion
            cargo-flamegraph
            clang
            gnuplot
            libxkbcommon
            lld
            mdbook
            pkg-config
            rust-toolchain
            stdenv.cc.cc.lib
            udev
            vulkan-loader
            vulkan-validation-layers
            wayland
            freetype
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
          ];

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
          VK_LAYER_PATH = "${pkgs.vulkan-validation-layers}/share/vulkan/explicit_layer.d/";

          RUST_SRC_PATH = "${rust-toolchain}/lib/rustlib/src/rust/library";
          RUST_LOG = "warn,wgpu_hal=off,wgpu_core=off,bevy=info,rg=debug";
          RUST_BACKTRACE = 1;
        };
      }
    );
}
