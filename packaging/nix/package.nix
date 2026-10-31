# Derivation for nixpkgs (copy to pkgs/by-name/do/doknyu/package.nix, see packaging/README.md).
#
# `hash` and `cargoHash` are placeholders until the tag v0.1.0 exists; packaging/nix/pin-release.sh
# fills them in. flake.nix builds the same derivation from the local tree (src and cargoDeps
# overridden), which is how `scripts/nix-check.sh` tests it before the tag.
{
  lib,
  stdenv,
  rustPlatform,
  fetchFromGitHub,
  libxkbcommon,
  vulkan-loader,
  wayland,
  libx11,
  libxcursor,
  libxi,
  libxcb,
  libGL,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "doknyu";
  version = "0.1.0";

  src = fetchFromGitHub {
    owner = "RafaelProfMgz";
    repo = "doknyu";
    tag = "v${finalAttrs.version}";
    hash = lib.fakeHash;
  };

  cargoHash = lib.fakeHash;

  # Wayland, X11, xkbcommon, Vulkan and GL are loaded with dlopen (winit/wgpu), so the linker does
  # not record them: put them in the rpath by hand, as other winit/wgpu apps in nixpkgs do (halloy).
  postFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
    patchelf --add-rpath ${
      lib.makeLibraryPath [
        libGL
        libx11
        libxcb
        libxcursor
        libxi
        libxkbcommon
        vulkan-loader
        wayland
      ]
    } $out/bin/doknyu
  '';

  postInstall = ''
    local id=io.github.rafaelprofmgz.Doknyu size

    install -Dm644 "assets/$id.desktop" -t $out/share/applications
    install -Dm644 "assets/$id.metainfo.xml" -t $out/share/metainfo
    install -Dm644 "assets/icons/$id.svg" -t $out/share/icons/hicolor/scalable/apps
    for size in 16 24 32 48 64 128 256 512; do
      install -Dm644 "assets/icons/hicolor/''${size}x''${size}/apps/$id.png" \
        -t "$out/share/icons/hicolor/''${size}x''${size}/apps"
    done
    # The binary embeds IBM Plex (OFL-1.1) and Lucide (ISC); their texts ship with it.
    install -Dm644 LICENSE-MIT LICENSE-APACHE -t $out/share/licenses/doknyu
    install -Dm644 assets/fonts/LICENSES.md $out/share/licenses/doknyu/LICENSES-fonts.md
  '';

  meta = {
    description = "Desktop manager for Docker containers: logs, stats, volumes, networks and a terminal";
    homepage = "https://github.com/RafaelProfMgz/doknyu";
    changelog = "https://github.com/RafaelProfMgz/doknyu/releases/tag/v${finalAttrs.version}";
    license = with lib.licenses; [
      asl20
      mit
      ofl
      isc
    ];
    maintainers = [ ];
    mainProgram = "doknyu";
    platforms = lib.platforms.linux;
  };
})
