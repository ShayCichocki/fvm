mod artifact;
mod benchmark;
mod cli;
mod command_util;
mod firecracker;
mod framework;
mod fvm_aot;
mod guest_init;
mod rootfs;
mod toolchain;

use anyhow::Result;
use clap::Parser;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = cli::Cli::parse();
    cli::execute(cli)
}
