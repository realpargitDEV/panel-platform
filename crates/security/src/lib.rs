//! Cryptography primitives.
//!
//! Nothing here talks to the database or the filesystem. Keeping it that way
//! means the security-critical code can be read and tested in isolation, which
//! is the only realistic way to be confident in it.
//!
//! This crate used to also carry password hashing, session tokens, TLS identity
//! generation and login rate limiting. Those existed to protect a network
//! listener. The application no longer has one — it runs in a single process on
//! the user's own machine — so the code they protected is gone and they went
//! with it. What remains is what still has a job: encrypting secret environment
//! variable values at rest, and keeping plaintext secrets out of logs.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod encryption;
pub mod secret;

pub use encryption::{decrypt, encrypt, Ciphertext, EncryptionKey};
pub use secret::Secret;
