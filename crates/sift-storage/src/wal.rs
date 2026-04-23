//! WAL: bincode+CRC framed records with a 10ms/1MB group-commit ticker.

// Frame encode/decode lands in Task 3, the async writer in Task 4.
