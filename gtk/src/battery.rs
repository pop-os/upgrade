pub fn active() -> bool {
    upower_dbus::UPower::new(-1).and_then(|upower| upower.on_battery()).unwrap_or(false)
}
