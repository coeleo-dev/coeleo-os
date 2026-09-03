//! Coeleo `.coe` package: COE1 + name + version + ELF + Ed25519.
//! Signed region is every byte except the trailing 64-byte signature.

#![no_std]

use ed25519_dalek::{Signature, VerifyingKey};

pub const MAGIC: &[u8; 4] = b"COE1";
pub const NAME_MAX: usize = 8;
pub const HDR: usize = 21;
pub const SIG_LEN: usize = 64;
pub const PAYLOAD_MAX: usize = 256 * 1024;

pub const PUBKEY: &[u8; 32] = include_bytes!("../../../../tools/coe-pack/dev.pub");

#[derive(Clone, Copy)]
pub struct Pkg<'a> {
    pub name: &'a str,
    pub ver_major: u16,
    pub ver_minor: u16,
    pub payload: &'a [u8],
    pub signed: &'a [u8],
    pub signature: &'a [u8; SIG_LEN],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PkgErr {
    Truncated,
    Magic,
    Name,
    Size,
}

pub fn pubkey() -> &'static [u8; 32] {
    PUBKEY
}

pub fn parse(blob: &[u8]) -> Result<Pkg<'_>, PkgErr> {
    if blob.len() < HDR + SIG_LEN {
        return Err(PkgErr::Truncated);
    }
    if blob[..4] != *MAGIC {
        return Err(PkgErr::Magic);
    }
    let name_len = usize::from(blob[4]);
    if name_len == 0 || name_len > NAME_MAX {
        return Err(PkgErr::Name);
    }
    let name_bytes = &blob[5..5 + NAME_MAX];
    if name_bytes[name_len..].iter().any(|&b| b != 0) {
        return Err(PkgErr::Name);
    }
    let name = core::str::from_utf8(&name_bytes[..name_len]).map_err(|_| PkgErr::Name)?;
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err(PkgErr::Name);
    }
    let ver_major = u16::from_le_bytes(blob[13..15].try_into().unwrap());
    let ver_minor = u16::from_le_bytes(blob[15..17].try_into().unwrap());
    let payload_len = u32::from_le_bytes(blob[17..21].try_into().unwrap()) as usize;
    if payload_len == 0 || payload_len > PAYLOAD_MAX {
        return Err(PkgErr::Size);
    }
    let end = HDR.saturating_add(payload_len);
    if blob.len() != end.saturating_add(SIG_LEN) {
        return Err(PkgErr::Size);
    }
    let signature: &[u8; SIG_LEN] = blob[end..].try_into().map_err(|_| PkgErr::Size)?;
    Ok(Pkg {
        name,
        ver_major,
        ver_minor,
        payload: &blob[HDR..end],
        signed: &blob[..end],
        signature,
    })
}

pub fn verify(pkg: &Pkg<'_>, pubkey: &[u8; 32]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(pubkey) else {
        return false;
    };
    let sig = Signature::from_bytes(pkg.signature);
    vk.verify_strict(pkg.signed, &sig).is_ok()
}
