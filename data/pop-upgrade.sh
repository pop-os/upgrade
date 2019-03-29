#!/bin/sh

export DEBIAN_FRONTEND="noninteractive"

message () {
    echo "$2"
    test "$1" = "-i" && {
        plymouth message --text="$2"
    }
    test "$1" = "-f" && {
        plymouth update --status="failed"
        plymouth message --text="$2"
        plymouth update --status="normal"
    }
}

upgrade () {
    percent=0

    # Watch progress of an update, and report it to the splash screen
    env LANG=C apt-get full-upgrade -y --allow-downgrades --show-progress \
        | while read -r line; do
            if test "Progress: [" = "$(echo ${line} | cut -c-11)"; then
                percent=$(echo ${line} | cut -c12-14)
                if test -n "${percent}"; then
                    plymouth system-update --progress=${percent}
                fi
            fi

            if test "Unpacking" = "$(echo ${line} | cut -c-9)" \
                || test "Setting up" = "$(echo ${line} | cut -c-10)" \
                || test "Processing triggers for" = "$(echo ${line} | cut -c-23)"
            then
                message -i "Installing Updates (${percent}%): $line"
            fi
        done

    # Validate the exit status
    apt-get full-upgrade -y --allow-downgrades
}

dpkg_configure () {
    dpkg --configure -a | while read -r line; do
        message -i "Upgrade failed: attempting to repair: $line"
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
    if upgrade || attempt_repair; then
        rm /pop-upgrade /system-update
        message -i "Upgrade complete, now rebooting the system"
        sleep 6
        systemctl reboot
    else
        message -f "Upgrade failed. Dropping to a shell for recovery"
        sleep 6
        systemctl rescue
    fi
}

FIRST_ATTEMPT=/upgrade-attempt1
SECOND_ATTEMPT=/upgrade-attempt2

if test -f $FIRST_ATTEMPT; then
    message -i "System rebooted without completing the uprade. Trying a second time"
    rm $FIRST_ATTEMPT
    sleep 6
    attempt_upgrade $SECOND_ATTEMPT
elif test -f $SECOND_ATTEMPT; then
    message -i "System failed to upgrade. Bailing on upgrade attempt."
    rm /pop-upgrade /system-update $SECOND_ATTEMPT
    sleep 6
else
    attempt_upgrade $FIRST_ATTEMPT
fi
