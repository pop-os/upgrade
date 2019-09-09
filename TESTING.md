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

- [ ] The recovery partition is missing from `/etc/fstab`.
- [ ] The EFI partition is missing from `/etc/fstab`.
- [ ] UUID mount points are switched to PartUUID.
- [ ] Missing Pop repositories are re-added.
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
    - [ ] `pop-upgrade release upgrade systemd` uses systemd's `offline-update` service for the upgrade.
    - [ ] `pop-upgrade release upgrade -f` forces an upgrade, even if the next release is a development branch.
- [ ] `pop-upgrade status` returns a string describing the status of the daemon (ie: `inactive`).
- [ ] Incompatible repositories will display a prompt to request to keep or disable them.
- [ ] Events should be parsed and displayed in the terminal without errors

### GTK

This frontend shares much of the same client code as the CLI, so by testing this, you are also testing much of the same behaviors seen in the CLI. This is what the majority of users will interact with.

- [ ] Update widget appears in the "About" page in gnome-control-center.
- [ ] The "About" page in GNOME search results should mention OS upgrades.
- [ ] On a system without a new release, it should report that no new releases are available.
- [ ] Selecting "Refresh OS" should boot into the recovery partition and skip to the "Refresh OS" view.
- [ ] "Refresh OS" should not be an available option on a system without a recovery partition.
- [ ] Selecting "Download" should present a dialog with a changelog detailing key items in the new release.
    - [ ] Selecting "Download" on an upgradeable release with a recovery partition.
        - [ ] The recovery partition should be upgraded in this scenario.
        - [ ] On success, the system should reboot into the recovery partition in upgrade mode.
    - [ ] Selecting "Download" on an upgradeable release without a recovery partition.
        - [ ] On success, the reboot will proceed into offline updates mode.
- [ ] Once "Download" is clicked, a progress bar appears to display the current step and its progress.
    - [ ] When downloading an update, it should be possible to cancel it with a "Cancel" button.
        - [ ] After clicking the "Cancel", the system should still able to reboot and update software, as if the upgrade had never been initiated.
- [ ] Make sure buttons are styled correctly and the text on them is correct.
    - [ ] The "Refresh OS" button must have destructive styling.
    - [ ] The "Download" button must have suggested styling.
    - [ ] The "Cancel" button must have destructive styling.
- [ ] The latest version should be displayed as the upgrade (18.04 should upgrade to 19.04, not 18.10).
- [ ] Options for dismissing desktop notifications must be present on LTS releases.
- [ ] Dismissing an update on LTS shall prevent desktop notifications from appearing until the next release.

### Notifications

Similar to Firmware Manager, a systemd user timer will occasionally ask the upgrade daemon if a new release is available.

- [ ] If a release is available, a desktop notification will display with non-actionable text declaring that an update is available.
- [ ] On clicking the notification, this will open GNOME Settings to allow the user to either dismiss or perform the upgrade.
- [ ] Non-LTS releases should not be allowed to dismiss an upgrade.
- [ ] After dismissing, verify that the next upgrade creates a notification.
- [ ] Repeated notifications should be reasonably timed.
- [ ] Double-check all text and icons in the notification, including version numbers.
- [ ] The "Upgrade" button should still appear when notifications are dismissed.

### Plymouth

Critical for offline upgrades with systemd, the Plymouth theme presents information from the upgrade script executed at init to the user in a way that does not overwhelm them with information. If Plymouth is disabled, the user will simply see a black screen with a lot of scrolling text.

- [ ] When performing an offline upgrade with systemd, our Plymouth theme will actively show progress as it occurs.
- [ ] The Plymouth screen will show the Pop! logo, but it should not pulsate.
- [ ] Make sure all text is shown on the screen and can be read. Text should not spill off the edges of the screen.

### Systemd Upgrades

When the recovery partition does not exist, systemd's `offline-updates` service is used to perform a release upgrade. The following are guidelines for testing this functionality.

- [ ] Test on a variety of possible configurations in the wild:
    - [ ] With, and without, a recovery partition
    - [ ] Custom kernels
    - [ ] Multi-boot installations
    - [ ] RAID arrays
    - [ ] LVM on LUKS (encrypted)
    - [ ] UEFI and legacy BIOS
    - [ ] Upgradeable and incomatible PPAs
- [ ] NVIDIA testing:
    - [ ] Old cards that require the legacy driver.
    - [ ] Graphics switching, including changing graphics modes, not rebooting, and initiating the upgrade.
    - [ ] Upgrades with our CUDA packages.
- [ ] Networking tests:
    - [ ] Simulate a flaky network connection while downloading the update to test what happens with unreliable connectivity and failed downloads.
    - [ ] Test running `pop-upgrade release upgrade systemd` while not connected to internet, see how it fails.
- [ ] Release upgrades should always upgrade to the latest-available release.
- [ ] Verify that the recovery partition is accessible after the upgrade.
- [ ] Test upgrades on systems that have fully up-to-date packages, and ones that have updates available.
- [ ] Verify that the Plymouth screen looks correct:
    - The Pop! logo should appear, but should not pulsate.
    - Update progress should be shown as a percentage. 
    - All text should be readable on the screen, and should not spill off the edges.
- [ ] The OS name in the EFI boot menu should be updated to reflect the new release.
- [ ] Test upgrades that will prompt the user, and see how those prompts are handled (something like the "restart docker daemon?" prompts that appear during `do-release-upgrade`).

### Recovery Upgrades

When in the recovery partition, distinst and the installer do the work of `schroot`'ing into the installed OS, and executing the same upgrade script that is used by offline upgrades. However, it does so in an environment that allows the user to use the recovery environment while they wait for the upgrade to complete, and if errors are encountered, presents options for handling those errors.

- [ ] Validate that "Refresh OS" works for encrypted and non-encrypted installs.
- [ ] Validate that "Upgrade OS" also works for encrypted and non-encrypted installs.
    - [ ] Any error that could be fixed automatically, should be fixed automatically.
    - [ ] When an error that can't be automatically corrected occurs, display options to handle the error:
        - [ ] A rescue terminal will schroot into the install with a bash prompt.
        - [ ] When the terminal is closed, the upgrade will be re-attempted.
        - [ ] Refreshing the OS should be an option that may also be selected.
