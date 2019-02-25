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

message -i "performing system upgrade"
if apt full-upgrade -y --allow-downgrades; then
    rm /pop-upgrade /system-update
    message -i "upgrade complete, rebooting the system"
    systemctl reboot
else
    message -f "upgrade failed; dropping to a shell for recovery:"
    echo "  * type `reboot` and enter to restart the machine."
    echo "  * type `journalctl pop-upgrade-script` to step through upgrade logs"
    echo "  * System76 customers may create support tickets for help"
    echo "  * Community support is also available at https://chat.pop-os.org/"
    systemctl rescue
fi
