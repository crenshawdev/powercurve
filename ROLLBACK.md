# Rollback

## Switching back to power-profiles-daemon

Stop and disable powercurve:

```
sudo systemctl stop com.vintagetechie.PowerCurve.service
sudo systemctl disable com.vintagetechie.PowerCurve.service
```

Remove the package:

```
# Arch
sudo pacman -R powercurve-git

# Debian / Ubuntu / Pop!_OS
sudo apt remove powercurve
```

Install power-profiles-daemon:

```
# Arch
sudo pacman -S power-profiles-daemon

# Debian / Ubuntu / Pop!_OS
sudo apt install power-profiles-daemon
```

Start it:

```
sudo systemctl enable --now power-profiles-daemon
```

The package manager handles the conflict automatically. powercurve
declares Provides/Conflicts with power-profiles-daemon, so the
package manager will offer to remove one when you install the other.

## Downgrading to a previous powercurve version

On Arch, if the old package is still in your cache:

```
sudo pacman -U /var/cache/pacman/pkg/powercurve-git-<old-version>-x86_64.pkg.tar.zst
```

On Debian/Ubuntu/Pop!_OS, download the older .deb from the
[releases page](https://gitlab.com/vintagetechie/powercurve/-/releases)
and install it:

```
sudo apt install ./powercurve_<old-version>_amd64.deb
```

## Cleaning up state

powercurve stores two things outside the package:

```
/etc/powercurve/fan.toml     # fan curve config, generated on install
/var/lib/powercurve/profile  # last active power profile
```

Removing the package leaves these in place so your config survives
reinstalls. To wipe them:

```
sudo rm -rf /etc/powercurve /var/lib/powercurve
```

On Debian, `sudo apt purge powercurve` removes these automatically.
