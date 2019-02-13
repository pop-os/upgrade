#!/bin/sh

echo "performing system upgrade"
if apt full-upgrade -y; then
    rm /pop-upgrade /system-update
    echo "upgrade complete, rebooting the system"
    systemctl reboot
else
    echo "upgrade failed; dropping to a shell for recovery:"
    echo "  * type `reboot` and enter to restart the machine."
    echo "  * type `journalctl pop-upgrade-script` to step through upgrade logs"
    echo "  * System76 customers may create support tickets for help"
    echo "  * Community support is also available at https://chat.pop-os.org/"
    systemctl rescue
fi
