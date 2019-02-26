#!/bin/sh

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
    apt-get full-upgrade -y --allow-downgrades
}

attempt_repair () {
    dpkg --configure -a
    upgrade
}

message -i "Performing system upgrade. The system will restart once complete."
if upgrade || attempt_repair; then
    rm /pop-upgrade /system-update
    message -i "Upgrade complete, now rebooting the system"
    systemctl reboot
else
    message -f "Upgrade failed. Dropping to a shell for recovery"
    systemctl rescue
fi
