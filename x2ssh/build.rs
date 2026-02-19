use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../x2ssh-agent/");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let cargo = env::var("CARGO").expect("CARGO not set");
    let _target = env::var("TARGET").unwrap_or_default();

    // Build agent with release-agent profile (optimized for size)
    let status = Command::new(&cargo)
        .args([
            "build",
            "-p",
            "x2ssh-agent",
            "--profile",
            "release-agent",
            "--target-dir",
            &format!("{out_dir}/agent-build"),
        ])
        .status()
        .expect("Failed to execute cargo build for x2ssh-agent");

    if !status.success() {
        panic!("x2ssh-agent build failed");
    }

    // Find the built binary (directly in release-agent/ when no explicit target)
    let agent_path = Path::new(&out_dir)
        .join("agent-build")
        .join("release-agent")
        .join("x2ssh-agent");

    let dest = Path::new(&out_dir).join("x2ssh-agent");

    std::fs::copy(&agent_path, &dest).unwrap_or_else(|e| {
        panic!(
            "Failed to copy agent binary from {:?} to {:?}: {}",
            agent_path, dest, e
        )
    });

    println!(
        "cargo:rustc-env=X2SSH_AGENT_PATH={}",
        dest.to_str().unwrap()
    );
}
