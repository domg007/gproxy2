//! Cross-target randomness. One source for DEK/nonce/PKCE verifiers/ids so the
//! channel + crypto layers don't depend on a cipher crate re-exporting an RNG
//! (chacha20poly1305 0.11 dropped its `aead::OsRng` re-export). `getrandom`
//! uses the OS backend on native and the `js` backend on wasm.

/// Fill `buf` with cryptographically secure random bytes. Panics only if the
/// platform RNG is unavailable, which is unrecoverable for our use.
pub fn fill(buf: &mut [u8]) {
    getrandom::getrandom(buf).expect("platform RNG unavailable");
}

/// A fresh array of `N` random bytes.
pub fn bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    fill(&mut b);
    b
}
