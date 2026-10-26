Name: dosty
Version: @VERSION@
Release: 1
Summary: A quiet, native Markdown writing environment
License: GPL-3.0-or-later
URL: https://github.com/MoonlarkStudios/dosty
BuildArch: x86_64
# The binary was already built with the pinned Rust toolchain and release profile.
%global debug_package %{nil}
%global __strip /bin/true
Requires: libxkbcommon.so.0()(64bit)
Requires: libxkbcommon-x11.so.0()(64bit)
Requires: libwayland-client.so.0()(64bit)
Requires: libwayland-cursor.so.0()(64bit)
Requires: libwayland-egl.so.1()(64bit)
Requires: libX11.so.6()(64bit)
Requires: libX11-xcb.so.1()(64bit)
Requires: libxcb.so.1()(64bit)
Requires: libXcursor.so.1()(64bit)
Requires: libXi.so.6()(64bit)
Requires: libXrandr.so.2()(64bit)
Requires: libdbus-1.so.3()(64bit)
Requires: xdg-desktop-portal, xdg-utils, zenity
Recommends: xdg-desktop-portal-gtk

%description
Write with live formatting, tables, images, and themes, or switch to Markdown
source. This early release has incomplete accessibility and platform validation.

%install
mkdir -p %{buildroot}
cp -a %{dosty_stage}/. %{buildroot}/

%files
/usr/bin/dosty
/usr/share/applications/app.dosty.editor.desktop
/usr/share/icons/hicolor/256x256/apps/app.dosty.editor.png
%license /usr/share/licenses/dosty
