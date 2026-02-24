pkgname=vintagetechie-power-git
pkgver=1.2.9.r469.865486c
pkgrel=1
pkgdesc="Desktop power management with per-channel fan control for Thelio systems"
arch=('x86_64' 'aarch64')
url="https://codeberg.org/VintageTechie/vintagetechie-power"
license=('GPL-3.0-or-later')
depends=(
  'dbus'
)
makedepends=('cargo' 'git')
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
  export CARGO_TARGET_DIR=target
  make
}

package() {
  cd "vintagetechie-power"
  export CARGO_TARGET_DIR=target
  make DESTDIR="${pkgdir}" install
}
