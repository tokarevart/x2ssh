use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../x2ssh-agent/");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let cargo = env::var("CARGO").expect("CARGO not set");

    // Build agent with musl target for fully static binary
    // This produces a portable binary that can run on any Linux server
    let musl_target = "x86_64-unknown-linux-musl";

    let status = Command::new(&cargo)
        .args([
            "build",
            "-p",
            "x2ssh-agent",
            "--target",
            musl_target,
            "--profile",
            "release-agent",
            "--target-dir",
            &format!("{out_dir}/agent-build"),
        ])
        .env("RUSTFLAGS", "-C target-feature=+crt-static")
        .status()
        .expect("Failed to execute cargo build for x2ssh-agent");

    if !status.success() {
        panic!(
            "x2ssh-agent build failed. Ensure musl target is installed: rustup target add \
             x86_64-unknown-linux-musl"
        );
    }

    // Find the built binary (in musl target subdirectory)
    let agent_path = Path::new(&out_dir)
        .join("agent-build")
        .join(musl_target)
        .join("release-agent")
        .join("x2ssh-agent");

    println!(
        "cargo:rustc-env=X2SSH_AGENT_PATH={}",
        agent_path.to_str().unwrap()
    );
}
