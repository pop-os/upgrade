#!/bin/bash

export LANG=C
export DEBIAN_FRONTEND="noninteractive"

# Prevent apt sources from being reverted once this script launches
rm -rf /pop-upgrade /pop_preparing_release_upgrade

message () {
    plymouth message --text="system-updates"

    test "$1" = "-i" && {
        plymouth message --text="$2"
    }
    test "$1" = "-f" && {
        plymouth update --status="failed"
        plymouth message --text="$2"
        plymouth update --status="normal"
    }
}

efi_rename () {
    if test -d '/sys/firmware/efi/'; then
        current_bootnum="$(efibootmgr | grep BootCurrent | awk -F' ' '{print $2}')"
        new_label="$(cat /etc/os-release | grep PRETTY | awk -F'"' '{print $2}')"

        # Get the disk where the ESP resides, and the partition number of the ESP.
        efi_part="$(awk '$2 == "/boot/efi"' /proc/mounts | awk '{print $1}' | awk -F/ '{print $3}')"

        for block in /sys/block/*; do
            if test -e "${block}/${efi_part}"; then
                efi_disk="/dev/$(echo ${block} | cut -c 12-)"
                break
            fi
        done

        efi_num="$(cat /sys/class/block/${efi_part}/partition)"

        # Remove the current boot entry
        efibootmgr -b "${current_bootnum}" -B

        # Then add the new entry, with the new name
        efibootmgr -c -L "${new_label}" \
            -d "${efi_disk}" \
            -p "${efi_num}" \
            -l "\EFI\SYSTEMD\SYSTEMD-BOOTX64.EFI"
    fi
}

dpkg_configure () {
    dpkg --configure -a --force-overwrite | while read -r line; do
        message -i "Repairing packages: $line"
    done

    # Validate the exit status
    dpkg --configure -a
}

apt_install_fix () {
    message -i "Checking for package fixes..."
    env LANG=C apt-get -o Dpkg::Options::="--force-overwrite" \
        -o Dpkg::Options::="--force-confdef" \
        -o Dpkg::Options::="--force-confold" \
        -o Dpkg::Options::="--force-breaks" \
        -o Dpkg::Options::="--force-conflicts" \
        -o Dpkg::Options::="--force-depends" \
        -o Dpkg::Options::="--force-depends-version" \
        install -f -y --allow-downgrades --show-progress \
        --no-download --ignore-missing
}

apt_full_upgrade () {
    percent=0

    # Watch progress of an update, and report it to the splash screen
    env LANG=C apt-get -o Dpkg::Options::="--force-overwrite" \
        -o Dpkg::Options::="--force-confdef" \
        -o Dpkg::Options::="--force-confold" \
        -o Dpkg::Options::="--force-breaks" \
        -o Dpkg::Options::="--force-conflicts" \
        -o Dpkg::Options::="--force-depends" \
        -o Dpkg::Options::="--force-depends-version" \
        full-upgrade -y --allow-downgrades --show-progress \
        --no-download --ignore-missing | while read -r line; do
            if test "Progress: [" = "$(echo ${line} | cut -c-11)"; then
                percent=$(echo "${line}" | cut -c12-14)
                if test -n "${percent}"; then
                    plymouth system-update --progress="${percent}"
                fi
            fi

            prefix="Installing Updates (${percent//[[:space:]]/}%)"

            if test "Unpacking" = "$(echo ${line} | cut -c-9)"; then
                package="$(echo $line | awk '{print $2}')"
                message -i "$prefix: Unpacking $package..."
            elif test "Setting up" = "$(echo ${line} | cut -c-10)"; then
                package="$(echo $line | awk '{print $3}')"
                message -i "$prefix: Setting up $package..."
            elif test "Processing triggers for" = "$(echo ${line} | cut -c-23)"; then
                package="$(echo $line | awk '{print $4}')"
                message -i "$prefix: Processing triggers for $package..."
            else
                echo "$line"
            fi
        done

    # Validate the exit status
    apt-get -o Dpkg::Options::="--force-overwrite" \
        full-upgrade -y --allow-downgrades \
        --no-download --ignore-missing
}

candidate () {
    echo "$1"=$(apt-cache policy "$1" | grep Candidate | awk '{print $2}')
}

package_exists () {
    dpkg -s "$1" &>/dev/null
}

install_packages () {
    local args=("$@")
    env LANG=C apt-get -o Dpkg::Options::="--force-overwrite" \
        -o Dpkg::Options::="--force-confdef" \
        -o Dpkg::Options::="--force-confold" \
        -o Dpkg::Options::="--force-breaks" \
        -o Dpkg::Options::="--force-conflicts" \
        -o Dpkg::Options::="--force-depends" \
        -o Dpkg::Options::="--force-depends-version" \
        install -y --allow-downgrades --show-progress \
        --no-download --ignore-missing \
        "${args[@]}" | while read -r line; do
            if test "Progress: [" = "$(echo ${line} | cut -c-11)"; then
                percent=$(echo "${line}" | cut -c12-14)
                if test -n "${percent}"; then
                    plymouth system-update --progress="${percent}"
                fi
            fi

            prefix="Installing Prerequisites (${percent//[[:space:]]/}%)"

            if test "Unpacking" = "$(echo ${line} | cut -c-9)"; then
                package="$(echo $line | awk '{print $2}')"
                message -i "$prefix: Unpacking $package..."
            elif test "Setting up" = "$(echo ${line} | cut -c-10)"; then
                package="$(echo $line | awk '{print $3}')"
                message -i "$prefix: Setting up $package..."
            elif test "Processing triggers for" = "$(echo ${line} | cut -c-23)"; then
                package="$(echo $line | awk '{print $4}')"
                message -i "$prefix: Processing triggers for $package..."
            else
                echo "$line"
            fi
        done
}

upgrade () {
    apt_install_fix
    apt_full_upgrade
}

attempt_repair () {
    message -i "Repairing packages..."

    for (( i=0; i<10; ++i)); do
        if upgrade; then
            message -i "Repair succeeded. Resuming upgrade..."
            sleep 3
            break
        fi
    done
}

# Attempts the upgrade the system, and if the upgrade fails, tries to repair it.
attempt_upgrade () {
    plymouth change-mode --system-upgrade
    plymouth system-update --progress="0"
    message -i "Installing Updates (0%)"
    touch "$1"

    systemctl mask acpid pop-upgrade

    if (upgrade || attempt_repair); then
        rm -rf  /system-update "$1"

        if test "$(grep VERSION_ID= /etc/os-release | cut -d '"' -f 2)" = "24.04"; then
            # Set cosmic-greeter as the default display manager
            rm -f /etc/systemd/system/display-manager.service
            ln -sf /lib/systemd/system/cosmic-greeter.service /etc/systemd/system/display-manager.service
        fi

        message -i "Upgrade complete. Removing old kernels..."
        apt remove linux-image-*hwe*

        message -i "Upgrade complete. Autoremoving old packages..."
        apt-get autoremove -y

        apt-mark minimize-manual -y
        if test "$(grep VERSION_ID= /etc/os-release | cut -d '"' -f 2)" = "24.04"; then
            message -i "Upgrade complete. Replacing GNOME..."
            apt-get remove --autoremove -y ~nlanguage-pack-gnome ~ngnome-user-docs gdm3 gnome-bluetooth gnome-calendar \
                gnome-contacts gnome-online-miners gnome-orca gnome-shell ~ngnome-shell-extension gnome-themes-standard \
                gnome-tweaks gnome-control-center gnome-online-accounts-gtk+
        fi

        message -i "Upgrade complete. Updating initramfs for all kernels..."
        update-initramfs -c -k all
        plymouth system-update --progress="100"

        efi_rename
        message -i "Upgrade complete. Preparing to reboot..."
        rm -f /etc/systemd/system/{acpid,pop-upgrade}.service
        sync
        sleep 3
        message -i "Upgrade complete. Now rebooting..."
        sleep 2
        systemctl reboot
    else
        rm -f /etc/systemd/system/{acpid,pop-upgrade}.service
        sync
        message -f "Upgrade failed. Restarting the system to try again..."
        sleep 5
        systemctl rescue
    fi
}

ATTEMPTED=/upgrade-attempted

test -e "$ATTEMPTED" && (message -i "System rebooted before upgrade was completed. Trying again..."; sleep 6)
attempt_upgrade "$ATTEMPTED"
