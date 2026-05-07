# PowerCurve Manual Test Rig

Reproducible checks proving the `powerprofilectl` drop-in contract holds and SSH or inactive sessions are gated for state-changing methods. Required pass before any D-Bus surface change merges.

## Scope

Verifies the v0.4.0 D-Bus surface across three interfaces:

- `com.vintagetechie.PowerCurve` at `/com/vintagetechie/PowerCurve`
- `org.freedesktop.UPower.PowerProfiles` at `/org/freedesktop/UPower/PowerProfiles`
- `net.hadess.PowerProfiles` at `/net/hadess/PowerProfiles`

All three are on the system bus.

## Prerequisites

- Target machine running PowerCurve daemon under systemd (`systemctl status powercurve`)
- Logged-in desktop session (GNOME or KDE) for active-session checks
- Second machine or second user account for inactive-session checks
- `powerprofilectl` from `upower` package installed
- `busctl` from `systemd` installed
- `loginctl` available

Confirm the daemon is running before each section:

    systemctl is-active powercurve

Expected output: `active`

## Section 1: powerprofilectl active-session contract

Run as the regular logged-in desktop user. Not via `sudo`. Not from a TTY console.

### 1.1 List profiles

Command:

    powerprofilectl list

Expected: prints three profiles (`power-saver`, `balanced`, `performance`), one marked active. No auth prompt. Exit code 0.

### 1.2 Get current profile

Command:

    powerprofilectl get

Expected: prints one of `power-saver`, `balanced`, `performance`. No auth prompt. Exit code 0.

### 1.3 Set profile to performance

Command:

    powerprofilectl set performance

Expected: silent success. No auth prompt. Exit code 0. Verify with:

    powerprofilectl get

Expected output: `performance`.

### 1.4 Set profile to balanced

Command:

    powerprofilectl set balanced

Expected: silent success. No auth prompt. Exit code 0. Verify with:

    powerprofilectl get

Expected output: `balanced`.

### 1.5 Set profile to power-saver

Command:

    powerprofilectl set power-saver

Expected: silent success. No auth prompt. Exit code 0. Verify with:

    powerprofilectl get

Expected output: `power-saver`. Restore to `balanced` after this check.

### 1.6 Hold profile via launch

Command:

    powerprofilectl launch -p performance -- sleep 5

Expected: silent success, holds `performance` for 5 seconds, returns to prior profile after `sleep` exits. No auth prompt. Exit code 0. During the 5-second window, in another terminal:

    powerprofilectl get

Expected output during the hold: `performance`.

## Section 2: busctl direct invocation

These calls bypass `powerprofilectl` and exercise the raw D-Bus surface. Run as the regular logged-in desktop user from the active session unless the subsection states otherwise.

### 2.1 Primary interface: GetProfile

Command:

    busctl call com.vintagetechie.PowerCurve \
      /com/vintagetechie/PowerCurve \
      com.vintagetechie.PowerCurve \
      GetProfile

Pre-Phase-2 expected: `s "Balanced"` (or whichever profile is active). No prompt. Exit 0.
Post-Phase-2 expected (active session): identical output. No prompt. Exit 0.
Post-Phase-2 expected (inactive session): identical output. Read-only methods are not gated.

### 2.2 Primary interface: Performance (set profile)

Command:

    busctl call com.vintagetechie.PowerCurve \
      /com/vintagetechie/PowerCurve \
      com.vintagetechie.PowerCurve \
      Performance

Pre-Phase-2 expected: empty reply, exit 0, no prompt. Profile switches to performance.
Post-Phase-2 expected (active session): empty reply, exit 0, no prompt.
Post-Phase-2 expected (inactive session): polkit prompt for admin password, or `Authentication required` error if no agent.

Restore with:

    busctl call com.vintagetechie.PowerCurve \
      /com/vintagetechie/PowerCurve \
      com.vintagetechie.PowerCurve \
      Balanced

### 2.3 Primary interface: SetFanOverride

Command (active session, valid channel):

    busctl call com.vintagetechie.PowerCurve \
      /com/vintagetechie/PowerCurve \
      com.vintagetechie.PowerCurve \
      SetFanOverride sy "cpu_fan" 50

Replace `cpu_fan` with a real channel name from:

    busctl call com.vintagetechie.PowerCurve \
      /com/vintagetechie/PowerCurve \
      com.vintagetechie.PowerCurve \
      GetFanDuties

Pre-Phase-2 expected: empty reply, exit 0, no prompt, no validation of channel name.
Post-Phase-2 expected (active session): polkit prompt for admin password (gated tighter than profile switch via `auth_admin_keep`), or success if cached.
Post-Phase-2 expected (inactive session): polkit prompt or denied.
Post-Phase-2 expected (active session, invalid channel name): `org.freedesktop.DBus.Error.InvalidArgs: unknown channel 'fakefan'`.

### 2.4 Primary interface: ClearFanOverride

Command:

    busctl call com.vintagetechie.PowerCurve \
      /com/vintagetechie/PowerCurve \
      com.vintagetechie.PowerCurve \
      ClearFanOverride s "cpu_fan"

Pre-Phase-2 expected: empty reply, exit 0, no prompt.
Post-Phase-2 expected (active session): polkit prompt or cached success.
Post-Phase-2 expected (inactive session): polkit prompt or denied.

### 2.5 UPower compat: SetActiveProfile

Command:

    busctl set-property org.freedesktop.UPower.PowerProfiles \
      /org/freedesktop/UPower/PowerProfiles \
      org.freedesktop.UPower.PowerProfiles \
      ActiveProfile s "performance"

Pre-Phase-2 expected: empty reply, exit 0, no prompt. `powerprofilectl get` returns `performance`.
Post-Phase-2 expected (active session): empty reply, exit 0, no prompt.
Post-Phase-2 expected (inactive session): polkit prompt for admin password.

Restore:

    busctl set-property org.freedesktop.UPower.PowerProfiles \
      /org/freedesktop/UPower/PowerProfiles \
      org.freedesktop.UPower.PowerProfiles \
      ActiveProfile s "balanced"

### 2.6 UPower compat: HoldProfile

Command:

    busctl call org.freedesktop.UPower.PowerProfiles \
      /org/freedesktop/UPower/PowerProfiles \
      org.freedesktop.UPower.PowerProfiles \
      HoldProfile sss "performance" "manual test" "manual-test-app"

Pre-Phase-2 expected: returns `u <cookie>` (a uint32 cookie), exit 0, no prompt. `powerprofilectl get` returns `performance`.
Post-Phase-2 expected (active session): returns `u <cookie>`, exit 0, no prompt.
Post-Phase-2 expected (inactive session): polkit prompt for admin password.

Release the hold (substitute the cookie value returned above):

    busctl call org.freedesktop.UPower.PowerProfiles \
      /org/freedesktop/UPower/PowerProfiles \
      org.freedesktop.UPower.PowerProfiles \
      ReleaseProfile u <cookie>

### 2.7 Hadess compat: HoldProfile

Command:

    busctl call net.hadess.PowerProfiles \
      /net/hadess/PowerProfiles \
      net.hadess.PowerProfiles \
      HoldProfile sss "power-saver" "gnome-shell test" "org.gnome.Shell"

Signature is `sss` (profile, reason, app-id) per upstream power-profiles-daemon. If `Method "HoldProfile" with signature "sss" doesn't exist`, retry with `ssss`. Plan 05 records the working form.

Pre-Phase-2 expected: returns `u <cookie>`, exit 0, no prompt.
Post-Phase-2 expected (active session): returns `u <cookie>`, exit 0, no prompt.
Post-Phase-2 expected (inactive session): polkit prompt for admin password.

Release:

    busctl call net.hadess.PowerProfiles \
      /net/hadess/PowerProfiles \
      net.hadess.PowerProfiles \
      ReleaseProfile u <cookie>

### 2.8 Hadess compat: ActiveProfile property write

Command:

    busctl set-property net.hadess.PowerProfiles \
      /net/hadess/PowerProfiles \
      net.hadess.PowerProfiles \
      ActiveProfile s "performance"

Pre-Phase-2 expected: empty reply, exit 0, no prompt. `powerprofilectl get` returns `performance`.
Post-Phase-2 expected (active session): empty reply, exit 0, no prompt.
Post-Phase-2 expected (inactive session): polkit prompt for admin password.

## Section 3: Inactive-session reproduction

The active-session checks above prove no-prompt behaviour. This section reproduces the inactive-session path so the gate triggers can be confirmed end-to-end. Pre-Phase-2 every method here behaves the same as the active session (no gate exists yet). Post-Phase-2 the gated paths must prompt for an admin password or fail with `Authentication required`.

### 3.1 Confirm session state

Run from the target session shell:

    loginctl list-sessions
    loginctl show-session $XDG_SESSION_ID --property=Active --property=Remote --property=Type

Expected for an active local desktop session:

    Active=yes
    Remote=no
    Type=wayland   (or x11)

Expected for an SSH session:

    Active=no
    Remote=yes
    Type=tty

Expected for a `su -` session from a non-graphical TTY:

    Active=yes
    Remote=no
    Type=tty

The polkit `allow_active=yes` rule in Phase 2 distinguishes by `Active=yes AND Remote=no AND Type IN (wayland, x11)`. A bare TTY `su -` is technically Active but is not a graphical session; treat it as the conservative inactive-equivalent for these tests.

### 3.2 SSH from another machine

From a second machine, with the target user's password or key:

    ssh user@target-machine
    powerprofilectl set performance

Pre-Phase-2 expected: silent success.
Post-Phase-2 expected: prompt for admin password via the controlling terminal's polkit agent (`pkttyagent`), or `Error performing operation: org.freedesktop.PolicyKit1.Error.NotAuthorized` if no agent is registered.

To register a TTY polkit agent for the SSH shell:

    pkttyagent --process $$ &

Then retry the `powerprofilectl set performance` command. Expected: prompt appears in the SSH shell.

### 3.3 Local non-graphical TTY (Ctrl-Alt-F3)

Switch to a text console (e.g. tty3 via Ctrl-Alt-F3), log in as a different user (or the same user with a fresh login), then:

    powerprofilectl set performance

Pre-Phase-2 expected: silent success.
Post-Phase-2 expected: prompt for admin password (since `Type=tty`, not `wayland` or `x11`).

### 3.4 Same user via second graphical session

If the desktop supports multi-seat or you have a second X/Wayland session, switching to it makes the original session `Active=no`. Run from the original (now inactive) graphical session:

    powerprofilectl set performance

Pre-Phase-2 expected: silent success.
Post-Phase-2 expected: prompt for admin password.

This case is rare on single-user desktops; document the behaviour but skip the test if multi-seat is not configured.

### 3.5 systemd-run as detached user (alternative to SSH)

If a second machine is unavailable, simulate an inactive-session caller from the same machine:

    systemd-run --user --scope --setenv=DBUS_SESSION_BUS_ADDRESS \
      sh -c 'powerprofilectl set performance'

Pre-Phase-2 expected: silent success.
Post-Phase-2 expected: behaviour depends on whether systemd-run inherits the active-session credentials. If `loginctl show-session` for the resulting transient unit shows `Active=yes`, this does not reproduce the inactive case. Confirm session state before relying on this path.

The reliable inactive-session reproductions are SSH (3.2) and TTY (3.3). Use 3.5 only as a quick-check.

## Section 4: GNOME and KDE smoke checks

The desktop power profile widgets are the highest-visibility consumer of the `net.hadess.PowerProfiles` and `org.freedesktop.UPower.PowerProfiles` interfaces. Behaviour change here is what users notice first.

### 4.1 GNOME (Shell 45+, Settings)

Path: `Settings -> Power -> Power Mode` (also surfaced in the system tray quick toggles).

Steps:

1. Open Settings, navigate to Power.
2. Confirm three options visible: `Power Saver`, `Balanced`, `Performance`.
3. Click each in turn.

Pre-Phase-2 expected: instant switch, no prompt, the active radio button moves to the clicked option. `powerprofilectl get` from a terminal confirms the change.
Post-Phase-2 expected (active session): identical to pre-Phase-2. No prompt.
Post-Phase-2 expected (inactive or remote session): GNOME Shell on a remote display is uncommon, but if reproduced, expect the polkit prompt to surface via the desktop's polkit agent (`gnome-shell` itself, or `polkit-gnome-authentication-agent-1`).

GNOME Shell quick-toggle: open the system tray (top-right corner), click the Power Mode toggle. Expected behaviour matches the Settings panel.

### 4.2 KDE Plasma 6 (System Settings)

Path: `System Settings -> Power Management -> Energy Saving -> Profile` (also surfaced in the battery applet on the system tray).

Steps:

1. Open System Settings, navigate to Power Management, then Energy Saving.
2. Confirm the profile selector lists three options.
3. Switch each profile in turn via the dropdown or the battery applet.

Pre-Phase-2 expected: silent switch, no prompt, the dropdown reflects the new selection. `powerprofilectl get` confirms.
Post-Phase-2 expected (active session): identical to pre-Phase-2. No prompt.
Post-Phase-2 expected (inactive session): polkit prompt via `polkit-kde-authentication-agent-1`.

### 4.3 GNOME Shell hold-profile path

GNOME Shell holds `power-saver` automatically when battery is critically low. To trigger manually for verification:

    busctl call net.hadess.PowerProfiles \
      /net/hadess/PowerProfiles \
      net.hadess.PowerProfiles \
      HoldProfile sss "power-saver" "low-battery" "org.gnome.Shell"

Confirm via:

    powerprofilectl get

Pre-Phase-2 expected: returns `power-saver`. No prompt.
Post-Phase-2 expected (active session): returns `power-saver`. No prompt. (This is the upstream PPD contract for shell-driven holds.)

Release after the test (substitute returned cookie):

    busctl call net.hadess.PowerProfiles \
      /net/hadess/PowerProfiles \
      net.hadess.PowerProfiles \
      ReleaseProfile u <cookie>

### 4.4 Screenshot policy

Take screenshots only when expected output is visual and a text command cannot capture it (e.g. the radio button position in GNOME Settings). Save to `tests/screenshots/<section>-<step>.png`. Commit screenshots when they materially help reproduction. Skip when text observation suffices.

For Phase 1 baseline (Plan 05), screenshots are optional. Plan 05 records text outputs; reviewers can rerun the GUI section live if needed.

## Section 5: Pre-polkit baseline

Captured to `tests/baseline-pre-polkit.md` separately on hardware. See Plan 05 in phase plan.
