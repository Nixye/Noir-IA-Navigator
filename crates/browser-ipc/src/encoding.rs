//! Bound serialized JSON before touching the transport. This bounds logical
//! bytes and requested buffer capacity, not allocator rounding, the input
//! object, custom Serialize CPU work, string scanning, or global process RAM.
use serde::Serialize;
use std::io::{self, Write};

fn bounded_length(length: usize, additional: usize, limit: usize) -> io::Result<usize> {
    length
        .checked_add(additional)
        .filter(|&required| required <= limit)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "IPC frame exceeds limit"))
}

fn next_capacity(current: usize, required: usize, limit: usize) -> usize {
    // Saturation handles doubling overflow without requesting above the cap.
    current.saturating_mul(2).max(required).min(limit)
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    limit: usize,
    // Kept separately from Vec::capacity(): the allocator may round upward.
    requested_capacity: usize,
}

impl BoundedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            requested_capacity: 0,
        }
    }
}

impl Write for BoundedBuffer {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let required = bounded_length(self.bytes.len(), input.len(), self.limit)?;
        if required > self.requested_capacity {
            let target = next_capacity(self.requested_capacity, required, self.limit);
            // reserve_exact takes an increment relative to len, not capacity.
            // No automatic Vec growth is needed by extend_from_slice below.
            self.bytes
                .try_reserve_exact(target - self.bytes.len())
                .map_err(io::Error::other)?;
            self.requested_capacity = target;
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Serialization, budget and allocation errors leave the transport untouched.
/// Transport errors after serialization may leave a partial frame; there is no
/// rollback. Keep the version-7 JSON and u32 little-endian framing unchanged.
pub(super) fn write_message_with_limit<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
    limit: usize,
) -> io::Result<()> {
    let mut buffer = BoundedBuffer::new(limit.min(u32::MAX as usize));
    serde_json::to_writer(&mut buffer, value).map_err(|error| {
        io::Error::new(error.io_error_kind().unwrap_or(io::ErrorKind::Other), error)
    })?;
    let length = u32::try_from(buffer.bytes.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&buffer.bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::{Error, SerializeSeq};

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Write(Vec<u8>),
        Flush,
    }

    #[derive(Default)]
    struct Transport {
        calls: Vec<Call>,
        bytes: Vec<u8>,
        fail_after: Option<usize>,
        fail_flush: bool,
    }

    impl Write for Transport {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.calls.push(Call::Write(bytes.to_vec()));
            let available = self
                .fail_after
                .map_or(bytes.len(), |limit| limit.saturating_sub(self.bytes.len()));
            if available == 0 && !bytes.is_empty() {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "test transport"));
            }
            let count = bytes.len().min(available);
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.calls.push(Call::Flush);
            if self.fail_flush {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "test flush"));
            }
            Ok(())
        }
    }

    #[test]
    fn exact_json_budget_and_one_byte_over() {
        let mut exact = Transport::default();
        write_message_with_limit(&mut exact, &"abc", 5).unwrap();
        assert_eq!(&exact.bytes[4..], b"\"abc\"");
        let mut rejected = Transport::default();
        let error = write_message_with_limit(&mut rejected, &"abc", 4).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(rejected.calls.is_empty());
        assert!(rejected.bytes.is_empty());
        assert!(write_message_with_limit(&mut rejected, &0, 0).is_err());
        assert!(rejected.calls.is_empty());
    }

    #[test]
    fn counts_escaped_controls_ascii_and_utf8_serialized_bytes() {
        for value in [
            "plain ASCII",
            "\u{0}\n\r\t\u{1f}",
            "\"\\",
            "Ol\u{e1} \u{1f30d}",
        ] {
            let expected = serde_json::to_vec(value).unwrap();
            let mut transport = Transport::default();
            write_message_with_limit(&mut transport, &value, expected.len()).unwrap();
            assert_eq!(&transport.bytes[4..], expected);
            let mut rejected = Transport::default();
            assert_eq!(
                write_message_with_limit(&mut rejected, &value, expected.len() - 1)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
            assert!(rejected.calls.is_empty());
        }
    }

    #[test]
    fn multi_chunk_budget_rejects_whole_block_before_append() {
        let mut buffer = BoundedBuffer::new(7);
        buffer.write_all(b"abc").unwrap();
        buffer.write_all(b"de").unwrap();
        let capacity = buffer.requested_capacity;
        assert_eq!(
            buffer.write(b"fgh").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(buffer.bytes, b"abcde");
        assert_eq!(buffer.requested_capacity, capacity);
        buffer.write_all(b"fg").unwrap();
        assert_eq!(buffer.bytes, b"abcdefg");
        assert_eq!(
            buffer.write(b"h").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(buffer.write(b"").unwrap(), 0);
    }

    struct FailingSerialize;

    impl Serialize for FailingSerialize {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut sequence = serializer.serialize_seq(Some(2))?;
            sequence.serialize_element("already buffered")?;
            Err(S::Error::custom("intentional serialization failure"))
        }
    }

    #[test]
    fn serialization_failure_never_writes_or_flushes_transport() {
        let mut transport = Transport::default();
        let error = write_message_with_limit(&mut transport, &FailingSerialize, 128).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("intentional serialization failure")
        );
        assert!(transport.calls.is_empty());
        assert!(transport.bytes.is_empty());
    }

    #[test]
    fn header_body_flush_order_and_same_bytes_roundtrip() {
        let value = vec!["one", "\u{e9}\n", "three"];
        let json = serde_json::to_vec(&value).unwrap();
        let header = (json.len() as u32).to_le_bytes();
        let mut transport = Transport::default();
        write_message_with_limit(&mut transport, &value, json.len()).unwrap();
        assert_eq!(
            transport.calls,
            vec![
                Call::Write(header.to_vec()),
                Call::Write(json.clone()),
                Call::Flush
            ]
        );
        let mut expected = header.to_vec();
        expected.extend_from_slice(&json);
        assert_eq!(transport.bytes, expected);
        let decoded: Vec<String> = crate::read_message(&mut transport.bytes.as_slice()).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn partial_transport_errors_are_propagated_without_rollback() {
        for fail_after in [0, 2, 4, 6] {
            let mut transport = Transport {
                fail_after: Some(fail_after),
                ..Transport::default()
            };
            assert_eq!(
                write_message_with_limit(&mut transport, &"abc", 5)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::BrokenPipe
            );
            assert_eq!(transport.bytes.len(), fail_after);
            assert!(!transport.calls.contains(&Call::Flush));
        }
        let mut transport = Transport {
            fail_flush: true,
            ..Transport::default()
        };
        assert_eq!(
            write_message_with_limit(&mut transport, &"abc", 5)
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(transport.bytes.len(), 9);
        assert_eq!(transport.calls.last(), Some(&Call::Flush));
    }

    #[test]
    fn requested_capacity_is_geometric_and_never_exceeds_budget() {
        let mut buffer = BoundedBuffer::new(13);
        let mut requests = Vec::new();
        for _ in 0..13 {
            let previous = buffer.requested_capacity;
            buffer.write_all(b"x").unwrap();
            if previous != buffer.requested_capacity {
                requests.push(buffer.requested_capacity);
            }
            assert!(buffer.bytes.len() <= buffer.requested_capacity);
            assert!(buffer.requested_capacity <= 13);
            assert!(buffer.bytes.capacity() >= buffer.requested_capacity);
        }
        assert_eq!(requests, vec![1, 2, 4, 8, 13]);
        assert!(buffer.write(b"x").is_err());
        assert_eq!(buffer.requested_capacity, 13);
    }

    #[test]
    fn arithmetic_overflow_is_rejected_without_large_allocations() {
        assert_eq!(
            bounded_length(usize::MAX, 0, usize::MAX).unwrap(),
            usize::MAX
        );
        assert_eq!(
            bounded_length(usize::MAX, 1, usize::MAX)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert!(bounded_length(5, 3, 7).is_err());
        assert_eq!(
            next_capacity(usize::MAX / 2 + 1, usize::MAX, usize::MAX),
            usize::MAX
        );
        assert_eq!(next_capacity(8, 9, 13), 13);
        assert_eq!(next_capacity(0, 0, 0), 0);
    }
}
