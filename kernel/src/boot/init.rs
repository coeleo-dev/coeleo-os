//! Load `/sh` as init. Returns whether to respawn.

use crate::elfload;
use crate::fs::{self, FsError};
use crate::process::Outcome;
use crate::sched;

pub fn try_sh() -> bool {
    match fs::read_file("sh") {
        Ok(bytes) => match elfload::load(&bytes) {
            Ok(image) => matches!(sched::run_init(image), Outcome::Exited),
            Err(()) => false,
        },
        Err(
            FsError::NoFs | FsError::NotFound | FsError::IsDir | FsError::Io | FsError::NotText,
        ) => false,
        Err(_) => false,
    }
}
