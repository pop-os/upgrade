#!/bin/sh

export DEBIAN_FRONTEND="noninteractive"
export LANG=C

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
    # Watch progress of an update, and report it to the splash screen
    apt-get full-upgrade -y --allow-downgrades --show-progress \
        | while read -r line; do
            if test "Progress: [" = "$(echo ${line} | cut -c-11)"; then
                percent=$(echo ${line} | cut -c12-14)
                if test -n "${percent}"; then
                    plymouth system-update --progress=${percent}
                    message -i "Installing Updates (${percent}%): The system will restart once complete."
                fi
            fi
        done

    # Validate the exit status
    apt-get full-upgrade -y --allow-downgrades
}

attempt_repair () {
    dpkg --configure -a
    upgrade
}

message -i "Installing Updates (0%): The system will restart once complete."
if upgrade || attempt_repair; then
    rm /pop-upgrade /system-update
    message -i "Upgrade complete, now rebooting the system"
    systemctl reboot
else
    message -f "Upgrade failed. Dropping to a shell for recovery"
    systemctl rescue
fi
