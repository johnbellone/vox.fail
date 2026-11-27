//! `vox-props`: builds `data/props` from the prop recipes in `source/props`.
//!
//! `cargo run -p vox-props -- --source source/props --out data/props`

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut source = PathBuf::from("source/props");
    let mut out = PathBuf::from("data/props");
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--source" => &mut source,
            "--out" => &mut out,
            _ => {
                eprintln!("usage: vox-props [--source <folder>] [--out <folder>]");
                return ExitCode::from(2);
            }
        };
        match args.next() {
            Some(value) => *slot = PathBuf::from(value),
            None => {
                eprintln!("{flag} needs a folder");
                return ExitCode::from(2);
            }
        }
    }
    match vox_props::build(&source, &out) {
        Ok(report) => {
            for line in &report.warnings {
                eprintln!("warning: {line}");
            }
            println!("wrote {} props to {}", report.props, out.display());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
