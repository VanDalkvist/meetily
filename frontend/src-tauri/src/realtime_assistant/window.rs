#[cfg(test)]
mod tests {
    use super::{
        latest_sequence_id, render_for_prompt, total_chars, RollingTranscriptWindow,
    };
    use crate::realtime_assistant::types::TranscriptSegmentInput;

    fn segment(
        sequence_id: u64,
        text: &str,
        audio_start_time: Option<f64>,
        is_partial: bool,
    ) -> TranscriptSegmentInput {
        TranscriptSegmentInput {
            sequence_id,
            text: text.to_string(),
            source: "mic".to_string(),
            timestamp: format!("2026-05-31T10:00:{:02}Z", sequence_id),
            audio_start_time,
            audio_end_time: audio_start_time.map(|start| start + 1.0),
            is_partial,
        }
    }

    #[test]
    fn push_segments_ignores_partial_empty_and_duplicate_segments() {
        let mut window = RollingTranscriptWindow::default();

        let added = window.push_segments(vec![
            segment(1, "first", Some(0.0), false),
            segment(2, "draft", Some(1.0), true),
            segment(3, "   ", Some(2.0), false),
            segment(1, "duplicate", Some(3.0), false),
            segment(4, "second", Some(4.0), false),
        ]);

        assert_eq!(added, 2);
        assert_eq!(window.segments().len(), 2);
        assert_eq!(window.segments()[0].sequence_id, 1);
        assert_eq!(window.segments()[1].sequence_id, 4);
    }

    #[test]
    fn windowed_segments_uses_audio_start_time_and_preserves_order() {
        let mut window = RollingTranscriptWindow::default();
        window.push_segments(vec![
            segment(1, "old", Some(0.0), false),
            segment(2, "keep one", Some(110.0), false),
            segment(3, "keep two", Some(170.0), false),
        ]);

        let segments = window.windowed_segments(60);

        assert_eq!(
            segments.iter().map(|segment| segment.sequence_id).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(latest_sequence_id(&segments), Some(3));
    }

    #[test]
    fn windowed_segments_falls_back_to_last_50_without_timing() {
        let mut window = RollingTranscriptWindow::default();
        let segments = (1..=55)
            .map(|sequence_id| segment(sequence_id, "text", None, false))
            .collect();
        window.push_segments(segments);

        let windowed = window.windowed_segments(180);

        assert_eq!(windowed.len(), 50);
        assert_eq!(windowed.first().map(|segment| segment.sequence_id), Some(6));
        assert_eq!(windowed.last().map(|segment| segment.sequence_id), Some(55));
    }

    #[test]
    fn render_for_prompt_is_stable_trimmed_and_counts_chars() {
        let segments = vec![
            segment(1, "  first line  ", Some(0.0), false),
            segment(2, "second", Some(1.0), false),
        ];

        let rendered = render_for_prompt(&segments);

        assert_eq!(
            rendered,
            "[seq=1 source=mic time=2026-05-31T10:00:01Z] first line\n[seq=2 source=mic time=2026-05-31T10:00:02Z] second"
        );
        assert_eq!(total_chars(&segments), "  first line  ".len() + "second".len());
    }
}
