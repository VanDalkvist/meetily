#[cfg(test)]
mod tests {
    use super::{clean_json_response, render_user_prompt};

    #[test]
    fn render_user_prompt_requires_json_object_and_includes_transcript() {
        let rendered_transcript = "[seq=7 source=mic time=2026-05-31T10:00:00Z] Discuss pricing";

        let prompt = render_user_prompt(rendered_transcript);

        assert!(prompt.contains("JSON object"));
        assert!(prompt.contains("answer"));
        assert!(prompt.contains("questionsToAsk"));
        assert!(prompt.contains("segmentSequenceId"));
        assert!(prompt.contains(rendered_transcript));
    }

    #[test]
    fn clean_json_response_strips_fenced_json() {
        let cleaned = clean_json_response("```json\n{\"answer\":\"ok\"}\n```");

        assert_eq!(cleaned, "{\"answer\":\"ok\"}");
    }

    #[test]
    fn clean_json_response_strips_plain_fence() {
        let cleaned = clean_json_response("```\n{\"answer\":\"ok\"}\n```");

        assert_eq!(cleaned, "{\"answer\":\"ok\"}");
    }
}
