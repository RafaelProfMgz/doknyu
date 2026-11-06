# Homebrew formula for the tap RafaelProfMgz/homebrew-tap (copy to Formula/doknyu.rb, see
# packaging/README.md, "Homebrew"). Linux only: the app is a Docker desktop client built for
# Linux (Wayland/X11). `url` and `sha256` are set by packaging/homebrew/pin-release.sh.
class Doknyu < Formula
  desc "Desktop manager for Docker containers on Linux"
  homepage "https://github.com/RafaelProfMgz/doknyu"
  url "https://github.com/RafaelProfMgz/doknyu/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "436d15b81fd9b08035a8553590a89f1e1cd15b07ffa5770088ca0f7145ab8d11"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/RafaelProfMgz/doknyu.git", branch: "main"

  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  # Wayland, X11, xkbcommon and Vulkan are loaded with dlopen (winit/wgpu), so the linker does not
  # record them; Homebrew puts its dependencies in the rpath, which is what makes dlopen find them.
  depends_on "libx11"
  depends_on "libxcb"
  depends_on "libxcursor"
  depends_on "libxi"
  depends_on "libxkbcommon"
  depends_on :linux
  depends_on "vulkan-loader"
  depends_on "wayland"

  def install
    system "cargo", "install", *std_cargo_args

    id = "io.github.rafaelprofmgz.Doknyu"
    (share/"applications").install "assets/#{id}.desktop"
    (share/"metainfo").install "assets/#{id}.metainfo.xml"
    (share/"icons/hicolor/scalable/apps").install "assets/icons/#{id}.svg"
    [16, 24, 32, 48, 64, 128, 256, 512].each do |size|
      (share/"icons/hicolor/#{size}x#{size}/apps").install "assets/icons/hicolor/#{size}x#{size}/apps/#{id}.png"
    end
    # The binary embeds IBM Plex (OFL-1.1) and Lucide (ISC); their texts ship with it.
    (share/"licenses/doknyu").install "LICENSE-MIT", "LICENSE-APACHE"
    (share/"licenses/doknyu").install "assets/fonts/LICENSES.md" => "LICENSES-fonts.md"
  end

  def caveats
    <<~EOS
      doknyu talks to the Docker Engine API through /var/run/docker.sock (or $XDG_RUNTIME_DIR/docker.sock),
      so Docker has to be installed and running; your user needs access to the socket.

      To get the launcher and icon in your desktop menu, add Homebrew to the data dirs of your session:
        export XDG_DATA_DIRS="#{HOMEBREW_PREFIX}/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
      or link the desktop file:
        ln -s #{opt_share}/applications/io.github.rafaelprofmgz.Doknyu.desktop ~/.local/share/applications/
    EOS
  end

  test do
    assert_match "doknyu #{version}", shell_output("#{bin}/doknyu --version")
  end
end
