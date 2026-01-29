const OPEN_TAG: &str = "<proposed_plan>";
const CLOSE_TAG: &str = "</proposed_plan>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProposedPlanSegment {
    Normal(String),
    ProposedPlanStart,
    ProposedPlanDelta(String),
    ProposedPlanEnd,
}

#[derive(Debug, Default)]
pub(crate) struct ProposedPlanParser {
    in_plan: bool,
    detect_tag: bool,
    line_buffer: String,
}

impl ProposedPlanParser {
    pub(crate) fn new() -> Self {
        Self {
            detect_tag: true,
            ..Self::default()
        }
    }

    pub(crate) fn parse(&mut self, delta: &str) -> Vec<ProposedPlanSegment> {
        let mut segments = Vec::new();
        let mut run = String::new();

        for ch in delta.chars() {
            if self.detect_tag {
                if !run.is_empty() {
                    self.push_text(std::mem::take(&mut run), &mut segments);
                }
                self.line_buffer.push(ch);
                if ch == '\n' {
                    self.finish_line(&mut segments);
                    continue;
                }
                let slug = self.line_buffer.trim_start();
                if slug.is_empty() || is_tag_prefix(slug) {
                    continue;
                }
                // This line cannot be a tag line, so flush it immediately.
                let buffered = std::mem::take(&mut self.line_buffer);
                self.detect_tag = false;
                self.push_text(buffered, &mut segments);
                continue;
            }

            run.push(ch);
            if ch == '\n' {
                self.push_text(std::mem::take(&mut run), &mut segments);
                self.detect_tag = true;
            }
        }

        if !run.is_empty() {
            self.push_text(run, &mut segments);
        }

        segments
    }

    pub(crate) fn finish(&mut self) -> Vec<ProposedPlanSegment> {
        let mut segments = Vec::new();
        if !self.line_buffer.is_empty() {
            // The buffered line never proved to be a tag line.
            let buffered = std::mem::take(&mut self.line_buffer);
            self.push_text(buffered, &mut segments);
        }
        if self.in_plan {
            push_segment(&mut segments, ProposedPlanSegment::ProposedPlanEnd);
            self.in_plan = false;
        }
        self.detect_tag = true;
        segments
    }

    fn finish_line(&mut self, segments: &mut Vec<ProposedPlanSegment>) {
        let line = std::mem::take(&mut self.line_buffer);
        let without_newline = line.strip_suffix('\n').unwrap_or(&line);
        let slug = without_newline.trim_start().trim_end();

        if slug == OPEN_TAG {
            if !self.in_plan {
                push_segment(segments, ProposedPlanSegment::ProposedPlanStart);
                self.in_plan = true;
            }
            self.detect_tag = true;
            return;
        }

        if slug == CLOSE_TAG {
            if self.in_plan {
                push_segment(segments, ProposedPlanSegment::ProposedPlanEnd);
                self.in_plan = false;
            }
            self.detect_tag = true;
            return;
        }

        self.detect_tag = true;
        self.push_text(line, segments);
    }

    fn push_text(&self, text: String, segments: &mut Vec<ProposedPlanSegment>) {
        if self.in_plan {
            push_segment(segments, ProposedPlanSegment::ProposedPlanDelta(text));
        } else {
            push_segment(segments, ProposedPlanSegment::Normal(text));
        }
    }
}

fn is_tag_prefix(slug: &str) -> bool {
    OPEN_TAG.starts_with(slug) || CLOSE_TAG.starts_with(slug)
}

fn push_segment(segments: &mut Vec<ProposedPlanSegment>, segment: ProposedPlanSegment) {
    match segment {
        ProposedPlanSegment::Normal(delta) => {
            if delta.is_empty() {
                return;
            }
            if let Some(ProposedPlanSegment::Normal(existing)) = segments.last_mut() {
                existing.push_str(&delta);
                return;
            }
            segments.push(ProposedPlanSegment::Normal(delta));
        }
        ProposedPlanSegment::ProposedPlanDelta(delta) => {
            if delta.is_empty() {
                return;
            }
            if let Some(ProposedPlanSegment::ProposedPlanDelta(existing)) = segments.last_mut() {
                existing.push_str(&delta);
                return;
            }
            segments.push(ProposedPlanSegment::ProposedPlanDelta(delta));
        }
        ProposedPlanSegment::ProposedPlanStart => {
            segments.push(ProposedPlanSegment::ProposedPlanStart);
        }
        ProposedPlanSegment::ProposedPlanEnd => {
            segments.push(ProposedPlanSegment::ProposedPlanEnd);
        }
    }
}

pub(crate) fn strip_proposed_plan_blocks(text: &str) -> String {
    let mut parser = ProposedPlanParser::new();
    let mut out = String::new();
    for segment in parser.parse(text).into_iter().chain(parser.finish()) {
        if let ProposedPlanSegment::Normal(delta) = segment {
            out.push_str(&delta);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::ProposedPlanParser;
    use super::ProposedPlanSegment;
    use super::strip_proposed_plan_blocks;
    use pretty_assertions::assert_eq;

    #[test]
    fn streams_proposed_plan_segments() {
        let mut parser = ProposedPlanParser::new();
        let mut segments = Vec::new();

        for chunk in [
            "Intro text\n<prop",
            "osed_plan>\n- step 1\n",
            "</proposed_plan>\nOutro",
        ] {
            segments.extend(parser.parse(chunk));
        }
        segments.extend(parser.finish());

        assert_eq!(
            segments,
            vec![
                ProposedPlanSegment::Normal("Intro text\n".to_string()),
                ProposedPlanSegment::ProposedPlanStart,
                ProposedPlanSegment::ProposedPlanDelta("- step 1\n".to_string()),
                ProposedPlanSegment::ProposedPlanEnd,
                ProposedPlanSegment::Normal("Outro".to_string()),
            ]
        );
    }

    #[test]
    fn preserves_non_tag_lines() {
        let mut parser = ProposedPlanParser::new();
        let mut segments = parser.parse("  <proposed_plan> extra\n");
        segments.extend(parser.finish());

        assert_eq!(
            segments,
            vec![ProposedPlanSegment::Normal(
                "  <proposed_plan> extra\n".to_string()
            )]
        );
    }

    #[test]
    fn closes_unterminated_plan_block_on_finish() {
        let mut parser = ProposedPlanParser::new();
        let mut segments = parser.parse("<proposed_plan>\n- step 1\n");
        segments.extend(parser.finish());

        assert_eq!(
            segments,
            vec![
                ProposedPlanSegment::ProposedPlanStart,
                ProposedPlanSegment::ProposedPlanDelta("- step 1\n".to_string()),
                ProposedPlanSegment::ProposedPlanEnd,
            ]
        );
    }

    #[test]
    fn strips_proposed_plan_blocks_from_text() {
        let text = "before\n<proposed_plan>\n- step\n</proposed_plan>\nafter";
        assert_eq!(strip_proposed_plan_blocks(text), "before\nafter");
    }
}
