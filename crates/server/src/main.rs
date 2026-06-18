use std::env;
use std::process;

use syntax_bridge_server::server::{DEFAULT_ADDR, run_blocking};

fn main() {
    let addr = server_addr();

    if let Err(error) = run_blocking(&addr) {
        eprintln!("syntax-bridge-server failed: {error}");
        process::exit(1);
    }
}

fn server_addr() -> String {
    let mut args = env::args().skip(1);
    let mut addr =
        env::var("SYNTAX_BRIDGE_SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_owned());

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => {
                let Some(value) = args.next() else {
                    eprintln!("usage: syntax-bridge-server [--addr HOST:PORT]");
                    process::exit(64);
                };
                addr = value;
            }
            "--help" | "-h" => {
                println!("usage: syntax-bridge-server [--addr HOST:PORT]");
                process::exit(0);
            }
            _ => {
                eprintln!("unknown argument: {arg}");
                eprintln!("usage: syntax-bridge-server [--addr HOST:PORT]");
                process::exit(64);
            }
        }
    }

    addr
}
