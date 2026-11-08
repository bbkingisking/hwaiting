fn main() {
    // SQLx embeds migrations at compile time; track the directory so new files trigger a rebuild.
    println!("cargo::rerun-if-changed=migrations");
}
