# Maintainer: Mark Wagie <mark dot wagie at proton dot me>
# Contributor: tleydxdy <shironeko(at)waifu(dot)club>
pkgname=system76-power
pkgver=1.2.9
pkgrel=1
pkgdesc="System76 Power Management"
arch=('x86_64' 'aarch64')
url="https://codeberg.org/VintageTechie/system76-power-custom"
license=('GPL-3.0-or-later')
depends=(
  'dbus'
  'libusb'
  'polkit'
)
makedepends=('cargo' 'git')
optdepends=(
  'system76-acpi-dkms: only needed for systems using open firmware with kernels <5.16'
  'system76-dkms: needed for systems using proprietary firmware'
)
provides=('power-profiles-daemon')
install="$pkgname.install"
source=("git+ssh://git@codeberg.org/VintageTechie/system76-power-custom.git")
sha256sums=('SKIP')

prepare() {
  cd "system76-power-custom"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "system76-power-custom"
  CFLAGS+=" -ffat-lto-objects"
  export RUSTUP_TOOLCHAIN=stable
  export HIDAPI_LINK_FLAGS="-lhidapi-hidraw"
  make
}

package() {
  cd "system76-power-custom"
  export HIDAPI_LINK_FLAGS="-lhidapi-hidraw"
  make DESTDIR="${pkgdir}" install
}
