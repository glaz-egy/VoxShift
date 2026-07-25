//! Discord RPC wire framing — 設計書.md §6.1.1/§16.
//!
//! Frame layout: 8-byte little-endian header (`opcode: u32`, `length: u32`)
//! followed by `length` bytes of JSON payload.

/// §16: 1MB frame size cap.
pub const MAX_FRAME_LEN: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum OpCode {
    Handshake = 0,
    Frame = 1,
    Close = 2,
    Ping = 3,
    Pong = 4,
}

impl TryFrom<u32> for OpCode {
    type Error = FrameError;

    fn try_from(value: u32) -> Result<Self, FrameError> {
        match value {
            0 => Ok(OpCode::Handshake),
            1 => Ok(OpCode::Frame),
            2 => Ok(OpCode::Close),
            3 => Ok(OpCode::Ping),
            4 => Ok(OpCode::Pong),
            other => Err(FrameError::UnknownOpCode(other)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("unknown discord rpc opcode: {0}")]
    UnknownOpCode(u32),
    #[error("frame exceeds the {MAX_FRAME_LEN} byte limit: {0} bytes")]
    TooLarge(usize),
}

pub fn encode(opcode: OpCode, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(payload.len()));
    }
    let mut buf = Vec::with_capacity(8 + payload.len());
    buf.extend_from_slice(&(opcode as u32).to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    Ok(buf)
}

/// Incremental frame decoder — feed it arbitrary chunks via [`push`] and
/// pull completed frames out with [`try_read_frame`]. Pure/IO-free, so it's
/// directly unit-testable against split and concatenated input (§23.1).
#[derive(Default)]
pub struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn try_read_frame(&mut self) -> Result<Option<(OpCode, Vec<u8>)>, FrameError> {
        if self.buf.len() < 8 {
            return Ok(None);
        }
        let opcode_raw = u32::from_le_bytes(self.buf[0..4].try_into().unwrap());
        let len = u32::from_le_bytes(self.buf[4..8].try_into().unwrap()) as usize;
        if len > MAX_FRAME_LEN {
            return Err(FrameError::TooLarge(len));
        }
        if self.buf.len() < 8 + len {
            return Ok(None);
        }
        let opcode = OpCode::try_from(opcode_raw)?;
        let payload = self.buf[8..8 + len].to_vec();
        self.buf.drain(0..8 + len);
        Ok(Some((opcode, payload)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode_round_trips() {
        let frame = encode(OpCode::Frame, b"{\"hello\":true}").unwrap();
        let mut reader = FrameReader::new();
        reader.push(&frame);
        let (opcode, payload) = reader.try_read_frame().unwrap().unwrap();
        assert_eq!(opcode, OpCode::Frame);
        assert_eq!(payload, b"{\"hello\":true}");
    }

    #[test]
    fn split_receive_returns_none_until_complete() {
        let frame = encode(OpCode::Frame, b"hello world").unwrap();
        let mut reader = FrameReader::new();

        // Feed the header only.
        reader.push(&frame[..8]);
        assert!(reader.try_read_frame().unwrap().is_none());

        // Feed the payload in two more pieces.
        reader.push(&frame[8..8 + 5]);
        assert!(reader.try_read_frame().unwrap().is_none());

        reader.push(&frame[8 + 5..]);
        let (opcode, payload) = reader.try_read_frame().unwrap().unwrap();
        assert_eq!(opcode, OpCode::Frame);
        assert_eq!(payload, b"hello world");
    }

    #[test]
    fn continuous_receive_yields_each_frame_in_order() {
        let a = encode(OpCode::Frame, b"first").unwrap();
        let b = encode(OpCode::Frame, b"second").unwrap();
        let mut reader = FrameReader::new();
        reader.push(&a);
        reader.push(&b);

        let (_, p1) = reader.try_read_frame().unwrap().unwrap();
        assert_eq!(p1, b"first");
        let (_, p2) = reader.try_read_frame().unwrap().unwrap();
        assert_eq!(p2, b"second");
        assert!(reader.try_read_frame().unwrap().is_none());
    }

    #[test]
    fn oversized_length_header_is_rejected() {
        let mut reader = FrameReader::new();
        let mut bogus = Vec::new();
        bogus.extend_from_slice(&(OpCode::Frame as u32).to_le_bytes());
        bogus.extend_from_slice(&((MAX_FRAME_LEN + 1) as u32).to_le_bytes());
        reader.push(&bogus);
        assert!(matches!(reader.try_read_frame(), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn unknown_opcode_is_rejected() {
        let mut reader = FrameReader::new();
        let mut bogus = Vec::new();
        bogus.extend_from_slice(&99u32.to_le_bytes());
        bogus.extend_from_slice(&0u32.to_le_bytes());
        reader.push(&bogus);
        assert!(matches!(reader.try_read_frame(), Err(FrameError::UnknownOpCode(99))));
    }
}
