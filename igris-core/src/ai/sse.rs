//! Incremental Server-Sent Events parser.
//!
//! Bytes are buffered until a full event (terminated by a blank line) is
//! available, so multi-byte UTF-8 characters split across network chunks are
//! never corrupted.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk and return every event it completes.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((end, sep_len)) = find_event_end(&self.buf) {
            let raw: Vec<u8> = self.buf.drain(..end + sep_len).take(end).collect();
            if let Some(ev) = parse_event(&String::from_utf8_lossy(&raw)) {
                events.push(ev);
            }
        }
        events
    }

    /// Flush a trailing event that wasn't followed by a blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        let rest = std::mem::take(&mut self.buf);
        parse_event(&String::from_utf8_lossy(&rest))
    }
}

fn find_event_end(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

fn parse_event(raw: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data.push(value),
            _ => {}
        }
    }
    if event.is_none() && data.is_empty() {
        return None;
    }
    Some(SseEvent { event, data: data.join("\n") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_across_chunk_boundaries() {
        let mut p = SseParser::new();
        assert!(p.push(b"event: message_start\nda").is_empty());
        let evs = p.push(b"ta: {\"a\":1}\n\nevent: ping\ndata: {}\n\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("message_start"));
        assert_eq!(evs[0].data, "{\"a\":1}");
        assert_eq!(evs[1].event.as_deref(), Some("ping"));
    }

    #[test]
    fn keeps_multibyte_utf8_split_across_chunks_intact() {
        let mut p = SseParser::new();
        let full = "data: héllo ✓\n\n".as_bytes();
        let split = full.iter().position(|&b| b == 0xE2).unwrap() + 1; // inside "✓"
        assert!(p.push(&full[..split]).is_empty());
        let evs = p.push(&full[split..]);
        assert_eq!(evs[0].data, "héllo ✓");
    }

    #[test]
    fn handles_crlf_comments_and_multiline_data() {
        let mut p = SseParser::new();
        let evs = p.push(b": keep-alive\r\n\r\ndata: line1\r\ndata: line2\r\n\r\n");
        assert_eq!(evs, vec![SseEvent { event: None, data: "line1\nline2".into() }]);
    }

    #[test]
    fn finish_flushes_unterminated_event() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: [DONE]").is_empty());
        assert_eq!(p.finish().unwrap().data, "[DONE]");
        assert!(p.finish().is_none());
    }
}
