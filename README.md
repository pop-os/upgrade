# Pop! Upgrade

This project provides distribution release upgrade support within Pop!\_OS. It serves as a
comprehensive replacement for Ubuntu's `do-release-upgrade` support; performing all release
upgrades offline, via systemd's offline upgrade feature.

The project consists of three major components: the daemon, the CLI frontend, and the GTK widget.
The daemon performs all release upgrade activities on behalf of either the CLI interface, or the
GTK widget that is embedded into GNOME Settings. A systemd timer service is also included to
occasionally run the CLI frontend to check for release upgrades, and displays a desktop
notification to the user when updates are found.

This enables us to provide release upgrades which are much less prone to error. Known issues
which would prevent a successful release upgrade are automatically repaired before the release
upgrade is scheduled. Performing upgrades offline in a minimal environment prevents the system
and running applications from crashing during the upgrade.

This also solves the issue of discoverability; where many of our users have found themselves
to be in an EOL'd release, beyond the EOL date. Included is a service which checks for
available release upgrades for Pop!\_OS; when updates are found, desktop notifications
are displayed to the user until they decide to perform the release upgrade.

## Dbus API

When launched in daemon mode (requires root), a new Dbus service will be registered, with the
following details:

- Interface: `com.system76.PopUpgrade`
- Name: `com.system76.PopUpgrade`
- Path: `/com/system76/PopUpgrade`

## License

Licensed under the GNU General Public License, Version 3.0, ([LICENSE](LICENSE) or https://www.gnu.org/licenses/gpl-3.0.en.html)

### Contribution

Any contribution intentionally submitted for inclusion in the work by you, shall be licensed under the GNU GPLv3.

