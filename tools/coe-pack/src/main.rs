//! Host packer for Coeleo `.coe` packages (fase 14).
//!
//! `dev.key` is a demo seed, not a production secret. `dev.pub` is the 32-byte
//! Ed25519 public key baked into the `pkg` crate.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ed25519_dalek::{Signer, SigningKey};

/// Demo seed. Must match `tools/coe-pack/dev.key`.
const SEED: [u8; 32] = *b"coeleo-pkg-dev-key-not-prod!!!!\0";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!("usage: coe-pack pack <name> <elf> -o <out.coe>");
        eprintln!("       coe-pack bad <name> <elf> -o <out.coe>");
        eprintln!("       coe-pack write-pub");
        return ExitCode::from(2);
    };
    match cmd.as_str() {
        "write-pub" => {
            if let Err(e) = write_pub() {
                eprintln!("coe-pack: {e}");
                return ExitCode::from(1);
            }
        }
        "pack" | "bad" => {
            let Some(name) = args.next() else {
                eprintln!("coe-pack: missing name");
                return ExitCode::from(2);
            };
            let Some(elf) = args.next() else {
                eprintln!("coe-pack: missing elf");
                return ExitCode::from(2);
            };
            let Some(dash_o) = args.next() else {
                eprintln!("coe-pack: missing -o");
                return ExitCode::from(2);
            };
            if dash_o != "-o" {
                eprintln!("coe-pack: expected -o <out>");
                return ExitCode::from(2);
            }
            let Some(out) = args.next() else {
                eprintln!("coe-pack: missing output path");
                return ExitCode::from(2);
            };
            let corrupt = cmd == "bad";
            if let Err(e) = pack(&name, Path::new(&elf), Path::new(&out), corrupt) {
                eprintln!("coe-pack: {e}");
                return ExitCode::from(1);
            }
        }
        _ => {
            eprintln!("coe-pack: unknown command {cmd}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn signing_key() -> SigningKey {
    let key_path = crate_dir().join("dev.key");
    let seed = fs::read(&key_path).ok().and_then(|b| {
        let a: [u8; 32] = b.try_into().ok()?;
        Some(a)
    });
    SigningKey::from_bytes(&seed.unwrap_or(SEED))
}

fn write_pub() -> Result<(), String> {
    let vk = signing_key().verifying_key();
    let dest = crate_dir().join("dev.pub");
    fs::write(&dest, vk.as_bytes()).map_err(|e| e.to_string())?;
    let key_path = crate_dir().join("dev.key");
    if !key_path.exists() {
        fs::write(&key_path, SEED).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn pack(name: &str, elf: &Path, out: &Path, corrupt: bool) -> Result<(), String> {
    if name.is_empty() || name.len() > pkg::NAME_MAX {
        return Err("name must be 1..=8 chars".into());
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err("name must be [a-z0-9]".into());
    }
    let payload = fs::read(elf).map_err(|e| format!("read elf: {e}"))?;
    if payload.len() > pkg::PAYLOAD_MAX {
        return Err("elf too large".into());
    }
    let mut blob = Vec::with_capacity(pkg::HDR + payload.len() + pkg::SIG_LEN);
    blob.extend_from_slice(pkg::MAGIC);
    blob.push(name.len() as u8);
    let mut name_pad = [0u8; pkg::NAME_MAX];
    name_pad[..name.len()].copy_from_slice(name.as_bytes());
    blob.extend_from_slice(&name_pad);
    blob.extend_from_slice(&0u16.to_le_bytes());
    blob.extend_from_slice(&1u16.to_le_bytes());
    blob.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    blob.extend_from_slice(&payload);
    let sk = signing_key();
    let sig = sk.sign(&blob);
    let mut sig_bytes = sig.to_bytes();
    if corrupt {
        let last = sig_bytes.len() - 1;
        sig_bytes[last] ^= 1;
    }
    blob.extend_from_slice(&sig_bytes);
    fs::write(out, blob).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkg::{parse, verify};

    #[test]
    fn round_trip_and_bad() {
        let dir = std::env::temp_dir();
        let elf = dir.join("coe-pack-elf.bin");
        let good = dir.join("hello.coe");
        let bad = dir.join("bad.coe");
        fs::write(&elf, b"\x7fELFmini").unwrap();
        pack("hello", &elf, &good, false).unwrap();
        pack("hello", &elf, &bad, true).unwrap();
        let g = fs::read(&good).unwrap();
        let b = fs::read(&bad).unwrap();
        let pkg_g = parse(&g).expect("good parse");
        let pkg_b = parse(&b).expect("bad parse");
        assert_eq!(pkg_g.name, "hello");
        assert_eq!(pkg_g.payload, b"\x7fELFmini");
        assert!(verify(&pkg_g, pkg::pubkey()));
        assert!(!verify(&pkg_b, pkg::pubkey()));
    }
}
