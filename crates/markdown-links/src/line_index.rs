//! Byte offset to 1-based line number mapping.

pub(crate) struct LineIndex {
    newlines: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(contents: &str) -> Self {
        let newlines = contents
            .as_bytes()
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .map(|(offset, _)| offset)
            .collect();
        Self { newlines }
    }

    /// 1-based line of `byte_offset`; CRLF and LF both count one line break.
    pub(crate) fn line_number(&self, byte_offset: usize) -> usize {
        self.newlines.partition_point(|&pos| pos < byte_offset) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::LineIndex;

    #[test]
    fn counts_lf_and_crlf_lines() {
        let lf = LineIndex::new("a\nb\nc");
        assert_eq!([0, 2, 4].map(|offset| lf.line_number(offset)), [1, 2, 3]);
        let crlf = LineIndex::new("a\r\nb\r\nc");
        assert_eq!([0, 3, 6].map(|offset| crlf.line_number(offset)), [1, 2, 3]);
    }
}
