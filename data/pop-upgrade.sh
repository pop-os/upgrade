#!/bin/bash

export DEBIAN_FRONTEND="noninteractive"

message () {
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
        current_bootnum=$(efibootmgr | grep BootCurrent | awk -F' ' '{print $2}')
        new_label=$(cat /etc/os-release | grep PRETTY | awk -F'"' '{print $2}')

        # Get the disk where the ESP resides, and the partition number of the ESP.
        efi_part=$(awk '$2 == "/boot/efi"' /proc/mounts | awk '{print $1}' | awk -F/ '{print $3}')

        for block in /sys/block/*; do
            if test -e ${block}/${efi_part}; then
                efi_disk="/dev/$(echo ${block} | cut -c 12-)"
                break
            fi
        done

        efi_num=$(cat /sys/class/block/${efi_part}/partition)

        # Remove the current boot entry
        efibootmgr -b "${current_bootnum}" -B

        # Then add the new entry, with the new name
        efibootmgr -c -L "${new_label}" \
            -d "${efi_disk}" \
            -p "${efi_num}" \
            -l "\EFI\SYSTEMD\SYSTEMD-BOOTX64.EFI"
    fi
}

upgrade () {
    percent=0

    # Watch progress of an update, and report it to the splash screen
    env LANG=C apt-get -o Dpkg::Options::="--force-overwrite" \
        full-upgrade -y --allow-downgrades --show-progress \
        --no-download --ignore-missing | while read -r line; do
            if test "Progress: [" = "$(echo ${line} | cut -c-11)"; then
                percent=$(echo "${line}" | cut -c12-14)
                if test -n "${percent}"; then
                    plymouth system-update --progress=${percent}
                fi
            fi

            prefix="Installing Updates (${percent}%)"

            if test "Unpacking" = "$(echo ${line} | cut -c-9)"; then
                package=$(echo $line | awk '{print $2}')
                message -i "$prefix: Unpacking $package ..."
            elif test "Setting up" = "$(echo ${line} | cut -c-10)"; then
                package=$(echo $line | awk '{print $3}')
                message -i "$prefix: Setting up $package ..."
            elif test "Processing triggers for" = "$(echo ${line} | cut -c-23)"; then
                package=$(echo $line | awk '{print $4}')
                message -i "$prefix: Processing triggers for $package ..."
            fi
        done

    # Validate the exit status
    apt-get -o Dpkg::Options::="--force-overwrite" \
        full-upgrade -y --allow-downgrades \
        --no-download --ignore-missing
}

dpkg_configure () {
    dpkg --configure -a --force-overwrite | while read -r line; do
        message -i "Attempting to repair: $line"
    done

    # Validate the exit status
    dpkg --configure -a
}

attempt_repair () {
    message -i "Upgrade failed: attempting to repair"
    if dpkg_configure; then
        message -i "Repair succeeded. Resuming upgrade"
        sleep 3
        upgrade
    fi
}

attempt_upgrade () {
    message -i "Installing Updates (0%)"
    touch $1
    apt-mark hold pop-upgrade
    if upgrade || attempt_repair; then
        rm /pop-upgrade /system-update /pop_preparing_release_upgrade
        message -i "Upgrade complete. Now autoremoving old packages"
        sudo apt-get autoremove -y
        efi_rename
        message -i "Now rebooting"
        sleep 6
        status=0
    else
        message -f "Upgrade failed. Dropping to a shell for recovery"
        sleep 6
        status=1
    fi

    apt-mark unhold pop-upgrade
}

plymouth message --text="system-updates"

FIRST_ATTEMPT=/upgrade-attempt1
SECOND_ATTEMPT=/upgrade-attempt2

if test -f $FIRST_ATTEMPT; then
    rm /pop_upgrade.log
    message -i "System rebooted without completing the upgrade. Trying a second time"
    rm $FIRST_ATTEMPT
    sleep 6
    attempt_upgrade $SECOND_ATTEMPT
elif test -f $SECOND_ATTEMPT; then
    message -i "System failed to upgrade. Bailing on upgrade attempt."
    rm /pop-upgrade /system-update /pop_preparing_release_upgrade $SECOND_ATTEMPT
    sleep 6
else
    attempt_upgrade $FIRST_ATTEMPT
fi

plymouth message --text="system-updates-stop"

systemctl reboot
