const OPEN_TAG: &str = "<proposed_plan>";
const CLOSE_TAG: &str = "</proposed_plan>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProposedPlanSegment {
    Normal,
    ProposedPlanStart,
    ProposedPlanDelta,
    ProposedPlanEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedAgentDelta {
    pub(crate) segment: ProposedPlanSegment,
    pub(crate) delta: String,
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

    pub(crate) fn parse(&mut self, delta: &str) -> Vec<ParsedAgentDelta> {
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

    pub(crate) fn finish(&mut self) -> Vec<ParsedAgentDelta> {
        let mut segments = Vec::new();
        if !self.line_buffer.is_empty() {
            // The buffered line never proved to be a tag line.
            let buffered = std::mem::take(&mut self.line_buffer);
            self.push_text(buffered, &mut segments);
        }
        if self.in_plan {
            push_segment(
                &mut segments,
                ProposedPlanSegment::ProposedPlanEnd,
                String::new(),
            );
            self.in_plan = false;
        }
        self.detect_tag = true;
        segments
    }

    fn finish_line(&mut self, segments: &mut Vec<ParsedAgentDelta>) {
        let line = std::mem::take(&mut self.line_buffer);
        let without_newline = line.strip_suffix('\n').unwrap_or(&line);
        let slug = without_newline.trim_start().trim_end();

        if slug == OPEN_TAG {
            if !self.in_plan {
                push_segment(
                    segments,
                    ProposedPlanSegment::ProposedPlanStart,
                    String::new(),
                );
                self.in_plan = true;
            }
            self.detect_tag = true;
            return;
        }

        if slug == CLOSE_TAG {
            if self.in_plan {
                push_segment(
                    segments,
                    ProposedPlanSegment::ProposedPlanEnd,
                    String::new(),
                );
                self.in_plan = false;
            }
            self.detect_tag = true;
            return;
        }

        self.detect_tag = true;
        self.push_text(line, segments);
    }

    fn push_text(&self, text: String, segments: &mut Vec<ParsedAgentDelta>) {
        let segment = if self.in_plan {
            ProposedPlanSegment::ProposedPlanDelta
        } else {
            ProposedPlanSegment::Normal
        };
        push_segment(segments, segment, text);
    }
}

fn is_tag_prefix(slug: &str) -> bool {
    OPEN_TAG.starts_with(slug) || CLOSE_TAG.starts_with(slug)
}

fn push_segment(segments: &mut Vec<ParsedAgentDelta>, segment: ProposedPlanSegment, delta: String) {
    if delta.is_empty()
        && matches!(
            segment,
            ProposedPlanSegment::Normal | ProposedPlanSegment::ProposedPlanDelta
        )
    {
        return;
    }
    if let Some(last) = segments.last_mut()
        && last.segment == segment
        && matches!(
            segment,
            ProposedPlanSegment::Normal | ProposedPlanSegment::ProposedPlanDelta
        )
    {
        last.delta.push_str(&delta);
        return;
    }
    segments.push(ParsedAgentDelta { segment, delta });
}

pub(crate) fn strip_proposed_plan_blocks(text: &str) -> String {
    let mut parser = ProposedPlanParser::new();
    let mut out = String::new();
    for segment in parser.parse(text).into_iter().chain(parser.finish()) {
        if matches!(segment.segment, ProposedPlanSegment::Normal) {
            out.push_str(&segment.delta);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::ParsedAgentDelta;
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
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::Normal,
                    delta: "Intro text\n".to_string(),
                },
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::ProposedPlanStart,
                    delta: String::new(),
                },
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::ProposedPlanDelta,
                    delta: "- step 1\n".to_string(),
                },
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::ProposedPlanEnd,
                    delta: String::new(),
                },
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::Normal,
                    delta: "Outro".to_string(),
                },
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
            vec![ParsedAgentDelta {
                segment: ProposedPlanSegment::Normal,
                delta: "  <proposed_plan> extra\n".to_string(),
            }]
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
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::ProposedPlanStart,
                    delta: String::new(),
                },
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::ProposedPlanDelta,
                    delta: "- step 1\n".to_string(),
                },
                ParsedAgentDelta {
                    segment: ProposedPlanSegment::ProposedPlanEnd,
                    delta: String::new(),
                },
            ]
        );
    }

    #[test]
    fn strips_proposed_plan_blocks_from_text() {
        let text = "before\n<proposed_plan>\n- step\n</proposed_plan>\nafter";
        assert_eq!(strip_proposed_plan_blocks(text), "before\nafter");
    }
}
