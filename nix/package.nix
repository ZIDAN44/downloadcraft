# DownloadCraft — the desktop app (`downloadcraft`) and the CLI (`downloadcraft-cli`), built from
# this repository (from LightCraft's nix/package.nix):
#
#   nix build          # → ./result/bin/downloadcraft, ./result/bin/downloadcraft-cli
#
# Kept in step with the release workflow and packaging/linux/package.sh:
#   * `Cargo.lock` drives the dependencies (`cargoLock`), so there is no vendorHash to bump. The git
#     sources (storytold's egui fork) are fetched at the revision the lock file pins;
#   * the desktop file, hicolor icons and AppStream metadata are the same files the .deb/.rpm ship
#     (packaging/linux/);
#   * `doCheck` runs `cargo test --workspace`, as `cargo xtask ci` does. Tests that need the network
#     use a server on 127.0.0.1, which the sandbox allows.
#
# yt-dlp, FFmpeg and Deno (video pages) are separate programs DownloadCraft finds on the PATH or
# fetches when asked; nixpkgs has all three (`yt-dlp`, `ffmpeg`, `deno`).
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  makeWrapper,
  # Windowing/GPU libraries: needed at build time by the crates that link them, at run time by the
  # ones that open them with dlopen() (see `runtimeLibs`), and by the dev shell (flake.nix).
  wayland,
  wayland-scanner,
  libxkbcommon,
  libx11,
  libxcb,
  libxcursor,
  libxi,
  libxrandr,
  vulkan-loader,
  libglvnd,
  # rfd opens libdbus-1 at run time to reach the xdg-desktop-portal folder picker.
  dbus,
  # Validates the desktop file and the AppStream metadata we install (as CI does).
  desktop-file-utils,
  appstream,
  # Adds the driver search path (/run/opengl-driver/lib) to the binaries' RPATH so wgpu finds the
  # installed Vulkan driver on NixOS. Optional: harmless on nixpkgs without it.
  autoAddDriverRunpath ? null,
  # Release date for the AppStream metadata (YYYY-MM-DD); the flake derives it from its own commit.
  buildDate ? "1970-01-01",
}:

let
  root = ../.;

  # The version lives in exactly one place: `[workspace.package] version` in the root Cargo.toml.
  version = (builtins.fromTOML (builtins.readFile (root + "/Cargo.toml"))).workspace.package.version;

  isLinux = stdenv.hostPlatform.isLinux;

  # AppStream id: also the desktop file name, and the icon name in share/icons/hicolor.
  appId = "ai.storyteller.downloadcraft";

  # Libraries the binaries open at run time with dlopen(): winit loads libwayland-client,
  # libxkbcommon and libxcb, rfd libdbus-1, wgpu the Vulkan loader or libEGL. The wrapper below
  # points LD_LIBRARY_PATH here.
  runtimeLibs = lib.optionals isLinux [
    libxkbcommon
    wayland
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
    vulkan-loader
    libglvnd
    dbus
  ];

  binaries = [
    "downloadcraft"
    "downloadcraft-cli"
  ];
in
rustPlatform.buildRustPackage {
  pname = "downloadcraft";
  inherit version;

  src = lib.cleanSourceWith {
    src = lib.cleanSource root;
    # The Nix files are not part of the build: editing them must not rebuild any Rust. Nor is the
    # fuzz workspace.
    filter =
      path: _type:
      !(builtins.elem (baseNameOf path) [
        "flake.nix"
        "flake.lock"
        "nix"
        "fuzz"
      ]);
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
    # The git dependencies (storytold's egui fork) are pinned by revision in Cargo.lock.
    allowBuiltinFetchGit = true;
  };

  cargoBuildFlags = [
    "--locked"
    "-p"
    "downloadcraft"
    "-p"
    "downloadcraft-cli"
  ];

  cargoTestFlags = [ "--workspace" ];

  strictDeps = true;

  nativeBuildInputs = [
    pkg-config
  ]
  ++ lib.optionals isLinux [
    makeWrapper
    wayland-scanner
    desktop-file-utils
    appstream
  ]
  ++ lib.optionals (isLinux && autoAddDriverRunpath != null) [ autoAddDriverRunpath ];

  buildInputs = lib.optionals isLinux [
    wayland
    libxkbcommon
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
  ];

  postInstall = ''
    doc=$out/share/doc/downloadcraft
    install -Dm644 -t "$doc" README.md LICENSE-MIT LICENSE-APACHE ATTRIBUTION.md
  ''
  + lib.optionalString isLinux ''
    # Desktop integration: the files the .deb/.rpm/.AppImage install.
    install -Dm644 packaging/linux/${appId}.desktop $out/share/applications/${appId}.desktop
    cp -r assets/app-icon/hicolor $out/share/icons/
    install -d $out/share/metainfo
    substitute packaging/linux/${appId}.metainfo.xml.in \
      $out/share/metainfo/${appId}.metainfo.xml \
      --subst-var-by VERSION ${version} \
      --subst-var-by DATE ${buildDate}

    desktop-file-validate $out/share/applications/${appId}.desktop
    XDG_CACHE_HOME=$TMPDIR/.cache appstreamcli validate --no-net \
      $out/share/metainfo/${appId}.metainfo.xml

    # makeWrapper keeps the binaries working when the libs above are only dlopen'd.
    for bin in ${lib.concatStringsSep " " (map (b: "$out/bin/" + b) binaries)}; do
      wrapProgram "$bin" --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath runtimeLibs}"
    done
  '';

  passthru = {
    # Shared with the dev shell so `cargo run` inside `nix develop` finds the same dlopen'd libs.
    inherit runtimeLibs;
  };

  meta = {
    description = "Download manager: files on many connections, resumed, and video pages";
    longDescription = ''
      DownloadCraft downloads files on several connections at once, resumes them after a pause,
      a dropped connection or a restart, and downloads videos from pages such as YouTube through
      yt-dlp, at the quality picked. It ships with downloadcraft-cli for scripts.
    '';
    homepage = "https://github.com/ZIDAN44/downloadcraft";
    license = with lib.licenses; [
      mit
      asl20
    ];
    sourceProvenance = with lib.sourceTypes; [ fromSource ];
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
    mainProgram = "downloadcraft";
  };
}
