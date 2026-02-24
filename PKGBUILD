pkgname=vintagetechie-power-git
pkgver=1.2.9.r465.ae9b4e3
pkgrel=1
pkgdesc="Desktop power management with per-channel fan control for Thelio systems"
arch=('x86_64' 'aarch64')
url="https://codeberg.org/VintageTechie/vintagetechie-power"
license=('GPL-3.0-or-later')
depends=(
  'dbus'
  'hidapi'
  'libusb'
  'polkit'
)
makedepends=('cargo' 'git')
optdepends=(
  'system76-acpi-dkms: only needed for systems using open firmware with kernels <5.16'
  'system76-dkms: needed for systems using proprietary firmware'
)
provides=('system76-power' 'power-profiles-daemon')
conflicts=('system76-power' 'power-profiles-daemon')
backup=('etc/system76-power/fan.toml')
install="system76-power.install"
source=("git+https://codeberg.org/VintageTechie/vintagetechie-power.git")
sha256sums=('SKIP')

pkgver() {
  cd "vintagetechie-power"
  local _ver
  _ver=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
  printf "%s.r%s.%s" "$_ver" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}

prepare() {
  cd "vintagetechie-power"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "vintagetechie-power"
  CFLAGS+=" -ffat-lto-objects"
  export RUSTUP_TOOLCHAIN=stable
  export HIDAPI_LINK_FLAGS="-lhidapi-hidraw"
  make
}

package() {
  cd "vintagetechie-power"
  export HIDAPI_LINK_FLAGS="-lhidapi-hidraw"
  make DESTDIR="${pkgdir}" install
}
