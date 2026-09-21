//! Splits a byte stream into lines on `\n` **and** bare `\r` (progress bars redraw with a
//! lone CR, no LF). A `\r\n` pair is treated as a single break, not two.

#[derive(Default)]
pub struct LineSplitter {
    buf: Vec<u8>,
    /// True if the previous byte fed in was a `\r` we already flushed on.
    pending_cr: bool,
}

impl LineSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a chunk of bytes, returning every complete line found (without the
    /// delimiter). Incomplete trailing data is buffered for the next call.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in chunk {
            match byte {
                b'\n' => {
                    if self.pending_cr {
                        // Second half of a \r\n pair — already flushed on the \r.
                        self.pending_cr = false;
                    } else {
                        lines.push(self.take_line());
                    }
                }
                b'\r' => {
                    lines.push(self.take_line());
                    self.pending_cr = true;
                }
                _ => {
                    self.buf.push(byte);
                    self.pending_cr = false;
                }
            }
        }
        lines
    }

    /// Flushes any buffered partial line (call at EOF).
    pub fn finish(mut self) -> Option<String> {
        if self.buf.is_empty() {
            None
        } else {
            Some(self.take_line())
        }
    }

    fn take_line(&mut self) -> String {
        let line = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_lf() {
        let mut s = LineSplitter::new();
        assert_eq!(s.feed(b"hello\nworld\n"), vec!["hello", "world"]);
    }

    #[test]
    fn splits_on_bare_cr_progress_bar() {
        let mut s = LineSplitter::new();
        assert_eq!(
            s.feed(b"Downloading  10%\rDownloading  20%\rDownloading  30%\n"),
            vec!["Downloading  10%", "Downloading  20%", "Downloading  30%"]
        );
    }

    #[test]
    fn crlf_pair_is_one_break() {
        let mut s = LineSplitter::new();
        assert_eq!(s.feed(b"a\r\nb\r\n"), vec!["a", "b"]);
    }

    #[test]
    fn partial_line_buffered_until_finish() {
        let mut s = LineSplitter::new();
        assert_eq!(s.feed(b"partial"), Vec::<String>::new());
        assert_eq!(s.finish(), Some("partial".to_string()));
    }

    #[test]
    fn finish_on_clean_eof_is_none() {
        let mut s = LineSplitter::new();
        assert_eq!(s.feed(b"complete\n"), vec!["complete"]);
        assert_eq!(s.finish(), None);
    }

    #[test]
    fn feed_across_chunk_boundaries() {
        let mut s = LineSplitter::new();
        assert_eq!(s.feed(b"hel"), Vec::<String>::new());
        assert_eq!(s.feed(b"lo\nwor"), vec!["hello"]);
        assert_eq!(s.feed(b"ld\n"), vec!["world"]);
    }

    #[test]
    fn crlf_split_across_chunks() {
        // \r arrives in one chunk, \n in the next — must still collapse to one break.
        let mut s = LineSplitter::new();
        assert_eq!(s.feed(b"a\r"), vec!["a"]);
        assert_eq!(s.feed(b"\nb\r\n"), vec!["b"]);
    }
}
