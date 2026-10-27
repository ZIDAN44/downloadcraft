{
  description = "DownloadCraft — a download manager: files on many connections, resumed, and video pages";

  inputs = {
    # DownloadCraft needs edition 2024 and rustc ≥ 1.95, so the flake tracks unstable. Consumers on a
    # release channel can retarget it: `inputs.downloadcraft.inputs.nixpkgs.follows = "nixpkgs";`
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  # From LightCraft's flake.
  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      inherit (nixpkgs) lib;

      # No x86_64-darwin: nixos-unstable dropped it. On an Intel Mac, install the .dmg from the
      # GitHub release.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      # The AppStream metadata carries a release date: the flake source's own commit date, which is
      # reproducible; a tarball or dirty tree without one falls back to the epoch.
      buildDate =
        let
          d = self.lastModifiedDate or "";
        in
        if builtins.stringLength d >= 8 then
          "${builtins.substring 0 4 d}-${builtins.substring 4 2 d}-${builtins.substring 6 2 d}"
        else
          "1970-01-01";

      forAllSystems = f: lib.genAttrs systems (system: f (import nixpkgs { inherit system; }) system);
    in
    {
      packages = forAllSystems (
        pkgs: _:
        let
          downloadcraft = pkgs.callPackage ./nix/package.nix { inherit buildDate; };
        in
        {
          inherit downloadcraft;
          default = downloadcraft;
        }
      );

      # `nixpkgs.overlays = [ inputs.downloadcraft.overlays.default ]` → `pkgs.downloadcraft`.
      overlays.default = final: _prev: {
        downloadcraft = final.callPackage ./nix/package.nix { inherit buildDate; };
      };

      devShells = forAllSystems (
        pkgs: system:
        let
          downloadcraft = self.packages.${system}.downloadcraft;
        in
        {
          default = pkgs.mkShell {
            # The package's own native inputs, so `cargo build` in the shell needs no extra setup.
            inputsFrom = [ downloadcraft ];
            packages = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
            ];
            # `cargo run` dlopens the Vulkan loader and libxkbcommon like the packaged binary.
            LD_LIBRARY_PATH = lib.makeLibraryPath downloadcraft.passthru.runtimeLibs;
            RUST_BACKTRACE = "1";
          };
        }
      );

      checks = forAllSystems (
        pkgs: system: {
          downloadcraft = self.packages.${system}.downloadcraft;

          # `nix fmt` formats the flake; this keeps it formatted.
          formatting =
            pkgs.runCommand "downloadcraft-nix-formatting" { nativeBuildInputs = [ pkgs.nixfmt ]; }
              ''
                nixfmt --check ${./flake.nix} ${./nix}
                touch $out
              '';
        }
      );

      formatter = forAllSystems (pkgs: _: pkgs.nixfmt);
    };
}
