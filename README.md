# pop-upgrade

Utility for upgrading Pop!\_OS and its recovery partition to new releases. This tool will be used
as a replacement for Ubuntu's `do-release-upgrade` script. The goal is to be less error-prone,
ensuring that critical packages are retained on upgrade, and better integration with Pop!\_OS.

## Dbus API

When launched in daemon mode (requires root), a new Dbus service will be registered, with the
following details:

- Interface: `com.system76.PopUpgrade`
- Name: `com.system76.PopUpgrade`
- Path: `/com/system76/PopUpgrade`

### DBus Methods

- `FetchUpdates (additional_strings: as, download_only: b) -> (updates_available: b, completed: s, total: s)`
    - Creates a task which will fetch all available updates, including the additional packages.
    - If an update task is already in progress, `completed` and `total` will have non-zero values.
    - If `updates_available` returns `false`, then there are no packages to fetch.
    - Unless `download_only` is specified as `true`, the packages will also be installed.
- `RecoveryUpgradeByFile (path: s) -> (result: y)`
    - Creates a task which will upgrade the recovery partition via a file ath the `path`.
- `RecoveryUpgradeByRelease (version: s, arch: s, flags: q) -> (result: y)`
    - Creates a task which will upgrade the recovery partition via the release API, using the defined details.
    - If package updates are available, a `FetchUpdates` task will execute beforehand.
    - `how` defines how the recovery partition should be upgraded.
      - Possible options are `file` and `release`.
    - `version` defines the suite to fetch from (ie: `20.04`)
    - `arch` defines which variant of that version to fetch (ie: `nvidia`)
    - `flags` sets additional configuration parameters for the task
- `RefreshOS () -> (result: y)`
- `ReleaseCheck () -> (current: s, next: s, build: n)`
    - Quickly checks the `current` release, determines the `next` release, and states whether
    an update is `available` or not.
- `ReleaseUpgrade (how: q, from: s, to: s)`
    - Creates a task to initiate a distribution release upgrade.
    - The `from` defines which suite to upgrade from.
    - The `to` defines the suite to upgrade to.
    - The upgrade method performed is determined by the `how`.
        - `1` will use systemd to perform an offline upgrade.
        - `2` will use the recovery partition to perform an offline upgrade.
        - Any other value will result in an error.
- `ReleaseRepair ()`
  - Performs automatic repairs of any issues found which may impact system operation
    - The `/etc/fstab` file will be corrected if certain mounts are missing or are mounting by the wrong ID
    - Source lists will also be parsed and corrected if they are missing any critical repositories
- `Status () -> (status: q, sub_status: q)`
    - Reports the current status of the daemon, where zero indicates inactivity.
    - If that `status` has a `sub_status`, it will be set to a non-zero value.
    - The available statuses for the main status are:
        - `0`: Inactive,
        - `1`: Fetching Packages,
        - `2`: Recovery Upgrade,
        - `3`: Release Upgrade,
        - `4`: Package Upgrade
- `UpgradePackages ()`
    - Upgrades packages for the current release, similar to performing a non-interactive upgrade normally.

### DBus Signals

- `PackageFetchResult (status: q)`
  - Indicates that a `FetchUpdates` task completed
  - A status of `0` indicate success, whereas `1` indicates failure
- `PackageFetched (package: s, completed: u, total: u)`
  - An event that is triggered when a `FetchUpdates` task has fetched a package.
  - `package` refers to the name of the package that was fetched.
  - `completed` and `total` can be used to track the progress of the task.
- `PackageFetching (package: s)`
  - An event that is triggered when a `FetchUpdates` task has begun fetching a new package.
  - `package` refers to the name of the package that was fetched
- `PackageUpgrade (event: a{ss})`
    - The ADT is represented as a map of field-value pairs.
- `RecoveryDownloadProgress (progress: t, total: t)`
  - Tracks the progress of the recovery files being fetched
- `RecoveryUpgradeEvent (event: q)`
  - Notifies the client of a recovery upgrade event that has occurred
- `RecoveryUpgradeResult (result: y)`
  - Indicates the final result of the recovery upgrade process
- `ReleaseUpgradeEvent (event: q)`
  - Notifies the client of a release upgrade event that has occurred
- `ReleaseUpgradeResult (result: y)`
  - Indicates the final result of the recovery upgrade process

### Recovery Upgrade Event

- `Fetching` (`1`): fetching recovery files
- `Syncing` (`2`): syncing recovery files with recovery partition
- `Verifying` (`3`): verifying checksums of fetched files
- `Complete` (`4`): recovery partition upgrade completed
- `Failed` (`5`): recovery partition upgrade failed

### Release Upgrade Event

- `UpdatingPackageLists` (`1`): updating package lists for the current release
- `FetchingPackages` (`2`): fetching updated packages for the current release
- `UpgradingPackages` (`3`): upgrading packages for the current release
- `InstallingPackages` (`4`): ensuring that system-critical packages are isntalled
- `UpdatingSourceLists` (`5`): updating the source lists to the new release
- `FetchingPackagesForNewRelease` (`6`): fetching packages for the new release
- `AttemptingLiveUpgrade` (`7`): attempting live upgrade to the new release
- `AttemptingSystemdUnit` (`8`): creating a systemd unit for installing the new release
- `AttemptingRecovery` (`9`): setting up the recovery partition to install the new release
- `Success` (`10`): new release is ready to install
- `SuccessLive` (`11`): new release was successfully installed
- `Failure` (`12`): an error occurred while setting up the upgrade
