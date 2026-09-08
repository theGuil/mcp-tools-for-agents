use mcp_tools::domains::video::remove_silence::{parse_silences, speech_segments};

const STDERR: &str = "
[silencedetect @ 0x1] silence_start: 1.02
[silencedetect @ 0x1] silence_end: 1.98 | silence_duration: 0.96
[silencedetect @ 0x1] silence_start: 2.9
";

#[test]
fn test_parse_silences_closes_open_interval() {
    assert_eq!(parse_silences(STDERR, 3.0), vec![(1.02, 1.98), (2.9, 3.0)]);
}

#[test]
fn test_speech_segments_inverts_and_pads() {
    let segments = speech_segments(&[(1.0, 2.0)], 3.0, 0.2);
    let pairs: Vec<(f64, f64)> = segments.iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(pairs, vec![(0.0, 1.2), (1.8, 3.0)]);
}

#[test]
fn test_speech_segments_merges_overlaps() {
    let segments = speech_segments(&[(1.0, 1.3)], 3.0, 0.2);
    let pairs: Vec<(f64, f64)> = segments.iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(pairs, vec![(0.0, 3.0)]);
}

#[test]
fn test_speech_segments_all_silent() {
    assert!(speech_segments(&[(0.0, 3.0)], 3.0, 0.0).is_empty());
}
