//! Unguessable tokens and keys, read from the operating system's random source.

use std::io::Read;

/// `count` bytes from the kernel's random source.
pub fn bytes(count: usize) -> std::io::Result<Vec<u8>> {
    let mut buffer = vec![0u8; count];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buffer)?;
    Ok(buffer)
}

/// `count` random bytes as lowercase hex (`2 * count` characters).
pub fn hex(count: usize) -> std::io::Result<String> {
    Ok(bytes(count)?.iter().map(|b| format!("{:02x}", b)).collect())
}
