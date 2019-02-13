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

- `FetchUpdates (additional_strings: as) -> (updates_available: b, completed: s, total: s)`
    - Creates a task which will fetch all available updates, including the additional packages.
    - If an update task is already in progress, `completed` and `total` will have non-zero values.
    - If `updates_available` returns `false`, then there are no packages to fetch.
- `RecoveryUpgradeByFile (path: s)`
    - Creates a task which will upgrade the recovery partition via a file ath the `path`.
- `RecoveryUpgradeByRelease (version: s, arch: s, flags: q)`
    - Creates a task which will upgrade the recovery partition via the release API, using the defined details.
    - If package updates are available, a `FetchUpdates` task will execute beforehand.
    - `how` defines how the recovery partition should be upgraded.
      - Possible options are `file` and `release`.
    - `version` defines the suite to fetch from (ie: `20.04`)
    - `arch` defines which variant of that version to fetch (ie: `nvidia`)
    - `flags` sets additional configuration parameters for the task
- `ReleaseCheck () -> (current: s, next: s, available: b)`
    - Quickly checks the `current` release, determines the `next` release, and states whether
    an update is `available` or not.
- `ReleaseUpgrade (how: q, from: s, to: s)`
    - Creates a task to initiate a distribution release upgrade.
    - The `from` defines which suite to upgrade from.
    - The `to` defines the suite to upgrade to.
    - The upgrade method performed is determined by the `how`.
        - `1` will perform the upgrade in place, at the user's peril.
        - `2` will create a oneshot systemd init script.
        - `3` will set up Pop's recovery partition.
        - Any other value will result in an error.

### DBus Signals

- `AptPackageFetchResult (status: q)`
  - Indicates that a `FetchUpdates` task completed
  - A status of `0` indicate success, whereas `1` indicates failure
- `AptPackageFetched (package: s, completed: u, total: u)`
  - An event that is triggered when a `FetchUpdates` task has fetched a package.
  - `package` refers to the name of the package that was fetched.
  - `completed` and `total` can be used to track the progress of the task.
- `AptPackageFetching (package: s)`
  - An event that is triggered when a `FetchUpdates` task has begun fetching a new package.
  - `package` refers to the name of the package that was fetched.
- `RecoveryDownloadProgress (progress: t, total: t)`
  - Tracks the progress of the recovery files being fetched
- `RecoveryUpgradeResult (result: q)`
  - Indicates the final result of the recovery upgrade process
