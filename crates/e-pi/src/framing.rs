//! Strict LF-delimited JSONL decoding for Pi RPC stdout.

use std::fmt;

use serde_json::Value;

pub const DEFAULT_MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    Oversized { limit: usize },
    InvalidUtf8,
    InvalidJson(String),
    UnterminatedRecord,
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversized { limit } => write!(formatter, "Pi RPC record exceeds {limit} bytes"),
            Self::InvalidUtf8 => formatter.write_str("Pi RPC record is not valid UTF-8"),
            Self::InvalidJson(error) => write!(formatter, "invalid Pi RPC JSON: {error}"),
            Self::UnterminatedRecord => formatter.write_str("unterminated Pi RPC JSON record"),
        }
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug)]
pub struct JsonlDecoder {
    buffer: Vec<u8>,
    max_record_bytes: usize,
    failed: bool,
}

impl JsonlDecoder {
    pub fn new(max_record_bytes: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_record_bytes,
            failed: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Value, FrameError>> {
        if self.failed {
            return Vec::new();
        }
        self.buffer.extend_from_slice(bytes);
        let mut records = Vec::new();
        loop {
            let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') else {
                if self.buffer.len() > self.max_record_bytes {
                    self.failed = true;
                    records.push(Err(FrameError::Oversized {
                        limit: self.max_record_bytes,
                    }));
                }
                break;
            };
            if newline > self.max_record_bytes {
                self.failed = true;
                records.push(Err(FrameError::Oversized {
                    limit: self.max_record_bytes,
                }));
                break;
            }
            let mut line = self.buffer.drain(..=newline).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            let text = match std::str::from_utf8(&line) {
                Ok(text) => text,
                Err(_) => {
                    self.failed = true;
                    records.push(Err(FrameError::InvalidUtf8));
                    break;
                }
            };
            match serde_json::from_str(text) {
                Ok(value) => records.push(Ok(value)),
                Err(error) => {
                    self.failed = true;
                    records.push(Err(FrameError::InvalidJson(error.to_string())));
                    break;
                }
            }
        }
        records
    }

    pub fn finish(&mut self) -> Option<Result<Value, FrameError>> {
        if self.failed || self.buffer.is_empty() {
            return None;
        }
        self.failed = true;
        Some(Err(FrameError::UnterminatedRecord))
    }
}

impl Default for JsonlDecoder {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_RECORD_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_only_on_lf_and_preserves_unicode_separators() {
        let mut decoder = JsonlDecoder::new(1024);
        let input = "{\"text\":\"a\u{2028}b\u{2029}c\"}\n{\"ok\":true}\r\n";
        let records = decoder.push(input.as_bytes());
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].as_ref().unwrap()["text"], "a\u{2028}b\u{2029}c");
        assert_eq!(records[1].as_ref().unwrap()["ok"], true);
    }

    #[test]
    fn handles_chunked_records() {
        let mut decoder = JsonlDecoder::new(1024);
        assert!(decoder.push(br#"{"type":"agent_"#).is_empty());
        let records = decoder.push(b"start\"}\n");
        assert_eq!(records[0].as_ref().unwrap()["type"], "agent_start");
    }

    #[test]
    fn rejects_oversized_and_unterminated_records() {
        let mut decoder = JsonlDecoder::new(4);
        assert!(matches!(
            decoder.push(b"12345").as_slice(),
            [Err(FrameError::Oversized { limit: 4 })]
        ));

        let mut decoder = JsonlDecoder::new(32);
        decoder.push(b"{\"a\":1}");
        assert_eq!(decoder.finish(), Some(Err(FrameError::UnterminatedRecord)));
    }
}
