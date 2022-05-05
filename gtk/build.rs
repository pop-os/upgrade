fn main() {
    pkg_config::Config::new().probe("pop_system_updater_gtk").unwrap();
    println!("cargo:rerun-if-changed=build.rs")
}
