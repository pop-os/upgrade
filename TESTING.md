# Testing

This document provides a guideline for testing and verifying the expected behaviors of the project. When a patch is ready for testing, the checklists may be copied and marked as they are proven to be working.

## Logs

When testing, it is ideal to have a terminal actively open watching logs from the upgrade daemon. This may be done through either of:

- Launching the daemon in a terminal: `sudo systemctl stop pop-upgrade; sudo pop-upgrade daemon`
- Watching the logs from journald: `journalctl -f -u pop-upgrade`

Much information is also presented in the CLI frontend in a more user-friendly manner, as the daemon communicates events and errors through DBus signals. Ideally, everything that can help diagnose an upgrade error should be logged, so consider it an issue if an error cannot be resolved with the information currently logged.

## Checklists

Tasks for a tester to verify when approving a patch.

### Daemon

All frontends interact with the daemon, and therefore testing a frontend will also test the daemon. However, there are ways that you can alter the system environment to make it hostile towards the success of a release upgrade. These are scenarios that the daemon should be able to recover from.

- [ ] If the recovery partition is missing from `/etc/fstab`, add it.
- [ ] If the EFI partition is missing from `/etc/fstab`, add it as well.
- [ ] Alters mount points to mount via PartUUID instead of UUID.
- [ ] Adds Pop's PPAs if they are missing from apt sources.
- [ ] `pop-desktop` is installed if it is not installed.
- [ ] Interactively allows the user to handle incompatible PPAs.
- [ ] Handles the `us.archives` -> `old-releases` transition for EOL'd upgrades.

### CLI

Features which can be tested from the command line interface. Each command gives detailed output which is not seen in the GTK frontend. When testing, report any wordings or colors that could be improved to give the user a better experience when using the command line.

- [ ] `pop-upgrade recovery default-boot` boots into the recovery partition on the next boot.
- [ ] `pop-upgrade recovery upgrade` upgrades the recovery partition.
- [ ] `pop-upgrade release check` reports the current, next, and release availability.
- [ ] `pop-upgrade release refresh` boots into the recovery partition in refresh mode.
- [ ] `pop-upgrade release repair` fixes a number of common system issues that may prevent an upgrade.
- [ ] `pop-upgrade release update` is equivalent to `apt update && apt full-upgrade`, but much faster.
- [ ] `pop-upgrade release upgrade` updates the current release, and prepares for a release upgrade.
    - [ ] `pop-upgrade release upgrade recovery` performs the above in the recovery partition.
    - [ ] `pop-upgrade release upgrade offline` uses systemd's `offline-update` service for the upgrade.
    - [ ] `pop-upgrade release upgrade -f` forces an upgrade, even if the next release is a development branch.
- [ ] `pop-upgrade status` returns a string describing the status of the daemon (ie: `inactive`).

### GTK

This frontend shares much of the same client code as the CLI, so by testing this, you are also testing much of the same behaviors seen in the CLI. This is what the majority of users will interact with.

#### Visual

- [ ] The "Refresh OS" button must have destructive styling.
- [ ] The "Download" button must have suggested styling.
- [ ] Options for dismissing desktop notifications must be present.

#### Implementation

- [ ] Dismissing an update shall prevent desktop notifications from appearing until the next release.
- [ ] Selecting "Refresh OS" should boot into the recovery partition and skip to the "Refresh OS" view.
- [ ] "Refresh OS" should not be an available option on a system without a recovery partition.
- [ ] On a system without a new release, it should report that no new releases are available.
- [ ] Selecting "Download" should present a dialog with a changelog detailing key items in the new release.
    - [ ] Selecting "Download" on an upgradeable release with a recovery partition.
        - [ ] The recovery partition should be upgraded in this scenario.
        - [ ] On success, the system should reboot into the recovery partition in upgrade mode.
    - [ ] Selecting "Download" on an upgradeable release without a recovery partition.
        - [ ] On success, the reboot will proceed into offline updates mode.

### Notifications

Similar to Firmware Manager, a systemd user timer will occasionally ask the upgrade daemon if a new release is available.

- [ ] If a release is available, a desktop notification will display with non-actionable text declaring that an update is available.
- [ ] On clicking the notification, this will open GNOME Settings to allow the user to either dismiss or perform the upgrade.

### Plymouth

Critical for offline upgrades with systemd, the Plymouth theme presents information from the upgrade script executed at init to the user in a way that does not overwhelm them with information. If Plymouth is disabled, the user will simply see a black screen with a lot of scrolling text.

- [ ] When performing an offline upgrade with systemd, our Plymouth theme will actively show progress as it occurs.

### Distinst / Installer

When in the recovery partition, distinst and the installer do the work of `schroot`'ing into the installed OS, and executing the same upgrade script that is used by offline upgrades. However, it does so in an environment that allows the user to use the recovery environment while they wait for the upgrade to complete, and if errors are encountered, presents options for handling those errors.

- [ ] Validate that "Refresh OS" works for encrypted and non-encrypted installs.
- [ ] Validate that "Upgrade OS" also works for encrypted and non-encrypted installs.
    - [ ] Any error that could be fixed automatically, should be fixed automatically.
    - [ ] When an error that can't be automatically corrected occurs, display options to handle the error:
        - [ ] A rescue terminal will schroot into the install with a bash prompt.
        - [ ] When the terminal is closed, the upgrade will be re-attempted.
        - [ ] Refreshing the OS should be an option that may also be selected.