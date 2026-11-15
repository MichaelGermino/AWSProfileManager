//! Manual check: read an entry written by 1.x's keytar, and write one keytar can read.
//! node -e "require('keytar').setPassword('APM-compat-check','acct1','...')" first.
use apm_core::secrets::{Keychain, SecretStore};

fn main() {
    let k = Keychain::new("APM-compat-check");
    println!("read acct1: {:?}", k.get("acct1"));
    k.set("acct2", "written-by-rust-ü").unwrap();
    println!("wrote acct2");
}
