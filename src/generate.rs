//! Password generation. Random bytes come from /dev/urandom (rspass is
//! unix-only, like the release matrix), mapped into the charset with
//! rejection sampling so no character is more likely than another.

use anyhow::{Context, Result};
use std::io::Read;

/// pass(1)'s default charsets (`$CHARACTER_SET` / `$CHARACTER_SET_NO_SYMBOLS`).
const ALNUM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const SYMBOLS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

pub fn generate_password(length: usize, no_symbols: bool) -> Result<String> {
    let charset: Vec<char> = if no_symbols {
        ALNUM.chars().collect()
    } else {
        ALNUM.chars().chain(SYMBOLS.chars()).collect()
    };
    let n = charset.len();
    // Rejection sampling: only accept bytes below the largest multiple of n
    // that fits in a byte, so `byte % n` is uniform.
    let limit = 256 - (256 % n);
    let mut urandom = std::fs::File::open("/dev/urandom").context("failed to open /dev/urandom")?;
    let mut password = String::with_capacity(length);
    let mut buf = [0u8; 64];
    while password.len() < length {
        urandom.read_exact(&mut buf).context("failed to read /dev/urandom")?;
        for &b in &buf {
            if (b as usize) < limit {
                password.push(charset[b as usize % n]);
                if password.len() == length {
                    break;
                }
            }
        }
    }
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_requested_length() {
        for len in [1, 25, 100] {
            assert_eq!(generate_password(len, false).unwrap().len(), len);
            assert_eq!(generate_password(len, true).unwrap().len(), len);
        }
    }

    #[test]
    fn no_symbols_is_alphanumeric() {
        let pw = generate_password(500, true).unwrap();
        assert!(pw.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn passwords_differ() {
        assert_ne!(
            generate_password(25, false).unwrap(),
            generate_password(25, false).unwrap()
        );
    }
}
