const OPEN_TAG: &str = "<proposed_plan>";
const CLOSE_TAG: &str = "</proposed_plan>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TagSpec<T> {
    open: &'static str,
    close: &'static str,
    tag: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TaggedLineSegment<T> {
    Normal(String),
    TagStart(T),
    TagDelta(T, String),
    TagEnd(T),
}

/// Line-based tag parser that buffers each line until it can disprove a tag
/// prefix. This allows tags to be detected correctly while streaming.
#[derive(Debug, Default)]
struct TaggedLineParser<T>
where
    T: Copy + Eq,
{
    specs: Vec<TagSpec<T>>,
    active_tag: Option<T>,
    detect_tag: bool,
    line_buffer: String,
}

impl<T> TaggedLineParser<T>
where
    T: Copy + Eq,
{
    fn new(specs: Vec<TagSpec<T>>) -> Self {
        Self {
            specs,
            active_tag: None,
            detect_tag: true,
            line_buffer: String::new(),
        }
    }

    fn parse(&mut self, delta: &str) -> Vec<TaggedLineSegment<T>> {
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
                if slug.is_empty() || self.is_tag_prefix(slug) {
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

    fn finish(&mut self) -> Vec<TaggedLineSegment<T>> {
        let mut segments = Vec::new();
        if !self.line_buffer.is_empty() {
            let buffered = std::mem::take(&mut self.line_buffer);
            let without_newline = buffered.strip_suffix('\n').unwrap_or(&buffered);
            let slug = without_newline.trim_start().trim_end();

            if let Some(tag) = self.match_open(slug) {
                if self.active_tag.is_none() {
                    push_segment(&mut segments, TaggedLineSegment::TagStart(tag));
                    self.active_tag = Some(tag);
                } else {
                    self.push_text(buffered, &mut segments);
                }
            } else if let Some(tag) = self.match_close(slug) {
                if self.active_tag == Some(tag) {
                    push_segment(&mut segments, TaggedLineSegment::TagEnd(tag));
                    self.active_tag = None;
                } else {
                    self.push_text(buffered, &mut segments);
                }
            } else {
                // The buffered line never proved to be a tag line.
                self.push_text(buffered, &mut segments);
            }
        }
        if let Some(tag) = self.active_tag.take() {
            push_segment(&mut segments, TaggedLineSegment::TagEnd(tag));
        }
        self.detect_tag = true;
        segments
    }

    fn finish_line(&mut self, segments: &mut Vec<TaggedLineSegment<T>>) {
        let line = std::mem::take(&mut self.line_buffer);
        let without_newline = line.strip_suffix('\n').unwrap_or(&line);
        let slug = without_newline.trim_start().trim_end();

        if let Some(tag) = self.match_open(slug)
            && self.active_tag.is_none()
        {
            push_segment(segments, TaggedLineSegment::TagStart(tag));
            self.active_tag = Some(tag);
            self.detect_tag = true;
            return;
        }

        if let Some(tag) = self.match_close(slug)
            && self.active_tag == Some(tag)
        {
            push_segment(segments, TaggedLineSegment::TagEnd(tag));
            self.active_tag = None;
            self.detect_tag = true;
            return;
        }

        self.detect_tag = true;
        self.push_text(line, segments);
    }

    fn push_text(&self, text: String, segments: &mut Vec<TaggedLineSegment<T>>) {
        if let Some(tag) = self.active_tag {
            push_segment(segments, TaggedLineSegment::TagDelta(tag, text));
        } else {
            push_segment(segments, TaggedLineSegment::Normal(text));
        }
    }

    fn is_tag_prefix(&self, slug: &str) -> bool {
        self.specs
            .iter()
            .any(|spec| spec.open.starts_with(slug) || spec.close.starts_with(slug))
    }

    fn match_open(&self, slug: &str) -> Option<T> {
        self.specs
            .iter()
            .find(|spec| spec.open == slug)
            .map(|spec| spec.tag)
    }

    fn match_close(&self, slug: &str) -> Option<T> {
        self.specs
            .iter()
            .find(|spec| spec.close == slug)
            .map(|spec| spec.tag)
    }
}

fn push_segment<T>(segments: &mut Vec<TaggedLineSegment<T>>, segment: TaggedLineSegment<T>)
where
    T: Copy + Eq,
{
    match segment {
        TaggedLineSegment::Normal(delta) => {
            if delta.is_empty() {
                return;
            }
            if let Some(TaggedLineSegment::Normal(existing)) = segments.last_mut() {
                existing.push_str(&delta);
                return;
            }
            segments.push(TaggedLineSegment::Normal(delta));
        }
        TaggedLineSegment::TagDelta(tag, delta) => {
            if delta.is_empty() {
                return;
            }
            if let Some(TaggedLineSegment::TagDelta(existing_tag, existing)) = segments.last_mut()
                && *existing_tag == tag
            {
                existing.push_str(&delta);
                return;
            }
            segments.push(TaggedLineSegment::TagDelta(tag, delta));
        }
        TaggedLineSegment::TagStart(tag) => {
            segments.push(TaggedLineSegment::TagStart(tag));
        }
        TaggedLineSegment::TagEnd(tag) => {
            segments.push(TaggedLineSegment::TagEnd(tag));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanTag {
    ProposedPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProposedPlanSegment {
    Normal(String),
    ProposedPlanStart,
    ProposedPlanDelta(String),
    ProposedPlanEnd,
}

#[derive(Debug)]
pub(crate) struct ProposedPlanParser {
    parser: TaggedLineParser<PlanTag>,
}

impl ProposedPlanParser {
    pub(crate) fn new() -> Self {
        Self {
            parser: TaggedLineParser::new(vec![TagSpec {
                open: OPEN_TAG,
                close: CLOSE_TAG,
                tag: PlanTag::ProposedPlan,
            }]),
        }
    }

    pub(crate) fn parse(&mut self, delta: &str) -> Vec<ProposedPlanSegment> {
        self.parser
            .parse(delta)
            .into_iter()
            .map(map_plan_segment)
            .collect()
    }

    pub(crate) fn finish(&mut self) -> Vec<ProposedPlanSegment> {
        self.parser
            .finish()
            .into_iter()
            .map(map_plan_segment)
            .collect()
    }
}

fn map_plan_segment(segment: TaggedLineSegment<PlanTag>) -> ProposedPlanSegment {
    match segment {
        TaggedLineSegment::Normal(text) => ProposedPlanSegment::Normal(text),
        TaggedLineSegment::TagStart(PlanTag::ProposedPlan) => {
            ProposedPlanSegment::ProposedPlanStart
        }
        TaggedLineSegment::TagDelta(PlanTag::ProposedPlan, text) => {
            ProposedPlanSegment::ProposedPlanDelta(text)
        }
        TaggedLineSegment::TagEnd(PlanTag::ProposedPlan) => ProposedPlanSegment::ProposedPlanEnd,
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

pub(crate) fn extract_proposed_plan_text(text: &str) -> Option<String> {
    let mut parser = ProposedPlanParser::new();
    let mut plan_text = String::new();
    let mut saw_plan_block = false;
    for segment in parser.parse(text).into_iter().chain(parser.finish()) {
        match segment {
            ProposedPlanSegment::ProposedPlanStart => {
                saw_plan_block = true;
                plan_text.clear();
            }
            ProposedPlanSegment::ProposedPlanDelta(delta) => {
                plan_text.push_str(&delta);
            }
            ProposedPlanSegment::ProposedPlanEnd | ProposedPlanSegment::Normal(_) => {}
        }
    }
    saw_plan_block.then_some(plan_text)
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
    fn closes_tag_line_without_trailing_newline() {
        let mut parser = ProposedPlanParser::new();
        let mut segments = parser.parse("<proposed_plan>\n- step 1\n</proposed_plan>");
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
