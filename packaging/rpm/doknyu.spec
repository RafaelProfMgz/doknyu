# Source0 is the tarball of the release tag. Source1 (the vendored crates, for an offline build) is
# NOT in the repository: packaging/rpm/make-vendor.sh generates it, and packaging/rpm/pin-release.sh
# builds the SRPM that goes to COPR. scripts/rpm-check.sh does the same from the local tree.
%global app_id io.github.rafaelprofmgz.Doknyu

Name:           doknyu
Version:        0.1.0
Release:        1%{?dist}
Summary:        Desktop manager for Docker containers: logs, stats, volumes, networks and a terminal

# The project is MIT OR Apache-2.0; the binary embeds IBM Plex (OFL-1.1) and Lucide (ISC) and the
# vendored crates. LICENSE.dependencies (generated in %%build) has the per-crate breakdown.
License:        (MIT OR Apache-2.0) AND OFL-1.1 AND ISC AND CROSSCHECK_DEPS_LICENSE
URL:            https://github.com/RafaelProfMgz/doknyu
Source0:        %{url}/archive/refs/tags/v%{version}/%{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.xz

ExclusiveArch:  %{rust_arches}

BuildRequires:  cargo-rpm-macros >= 24
BuildRequires:  desktop-file-utils
BuildRequires:  appstream

# The GUI stack is loaded with dlopen (winit/wgpu), so rpm does not see it in the ELF: without
# these the window does not open.
Requires:       hicolor-icon-theme
Requires:       libwayland-client
Requires:       libwayland-cursor
Requires:       libxkbcommon
Requires:       libxkbcommon-x11
Requires:       libX11
Requires:       libXcursor
Requires:       libXi
Requires:       libxcb
Requires:       libglvnd-egl
Requires:       vulkan-loader
Recommends:     mesa-vulkan-drivers
# The daemon may be remote or rootless, so Docker is only a recommendation. The terminal tab
# does not need the docker CLI (doknyu --exec-bridge).
Recommends:     (moby-engine or docker-ce)

%description
doknyu is a fast, keyboard-friendly desktop app to manage Docker containers, logs, stats,
volumes and networks, with an embedded terminal for each container.

%prep
%autosetup -n %{name}-%{version} -a1
%cargo_prep -v vendor

%build
%cargo_build
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies
%{cargo_vendor_manifest}

%install
# Not %%cargo_install: the crate has a lib target, so the macro would also install the library
# sources under /usr/share/cargo/registry (they are for -devel packages, not for an application).
install -Dpm755 target/rpm/doknyu -t %{buildroot}%{_bindir}
install -Dpm644 assets/%{app_id}.desktop -t %{buildroot}%{_datadir}/applications/
install -Dpm644 assets/%{app_id}.metainfo.xml -t %{buildroot}%{_metainfodir}/
install -Dpm644 assets/icons/%{app_id}.svg -t %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/
for size in 16 24 32 48 64 128 256 512; do
    install -Dpm644 assets/icons/hicolor/${size}x${size}/apps/%{app_id}.png \
        -t %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/
done

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{app_id}.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml
%cargo_test

%files
%license LICENSE-MIT LICENSE-APACHE LICENSE.dependencies cargo-vendor.txt
%license assets/fonts/LICENSES.md
%doc README.md
%{_bindir}/doknyu
%{_datadir}/applications/%{app_id}.desktop
%{_metainfodir}/%{app_id}.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/%{app_id}.*

%changelog
* Fri Oct 30 2026 RafaelProfMgz <RafaelProfMgz@users.noreply.github.com> - 0.1.0-1
- Initial package
