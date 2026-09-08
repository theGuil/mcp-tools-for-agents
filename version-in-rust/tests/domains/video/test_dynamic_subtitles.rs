use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::burn_subtitles::{burn_subtitles, BurnOptions};
use mcp_tools::domains::video::dynamic_subtitles::{
    ass_color, ass_time, create_dynamic_subtitles, DynamicSubtitlesOptions, SubtitleStyle,
    WordTiming,
};

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

fn words() -> Vec<WordTiming> {
    [
        (0.0, 0.4, "olá"),
        (0.4, 0.8, "mundo"),
        (0.9, 1.3, "isso"),
        (1.3, 1.6, "é"),
        (1.6, 2.0, "teste"),
        (2.8, 2.95, "fim"),
    ]
    .into_iter()
    .map(|(start, end, word)| WordTiming {
        start,
        end,
        word: word.to_string(),
    })
    .collect()
}

#[test]
fn test_ass_helpers() {
    assert_eq!(ass_time(0.0), "0:00:00.00");
    assert_eq!(ass_time(3661.5), "1:01:01.50");
    assert_eq!(ass_color("#FFD700", "x").unwrap(), "&H0000D7FF");
}

#[test]
fn test_create_dynamic_subtitles_highlight() {
    let rt = runtime();
    let options = DynamicSubtitlesOptions {
        max_words: 4,
        max_gap: 0.5,
        ..DynamicSubtitlesOptions::default()
    };
    let result =
        create_dynamic_subtitles(&rt.runtime, &words(), "legendas/din.ass", &options).unwrap();
    assert_eq!(result.output, "legendas/din.ass");
    assert_eq!(result.words, 6);
    assert_eq!(result.groups, 3);
    assert_eq!(result.duration, 2.95);
    assert_eq!(result.play_res, "1080x1920");
    let content =
        std::fs::read_to_string(rt.runtime.workspace.existing("legendas/din.ass").unwrap())
            .unwrap();
    assert!(content.contains("PlayResX: 1080"));
    assert!(content.contains("{\\c&H0000D7FF&\\fscx108\\fscy108}OLÁ"));
    assert_eq!(content.matches("Dialogue:").count(), 6);
}

#[test]
fn test_create_dynamic_subtitles_word_and_block() {
    let rt = runtime();
    let word = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "w.ass",
        &DynamicSubtitlesOptions {
            style: SubtitleStyle::Word,
            uppercase: false,
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap();
    let content = std::fs::read_to_string(rt.runtime.workspace.existing("w.ass").unwrap()).unwrap();
    assert_eq!(word.words, 6);
    assert!(content.contains("olá"));
    assert!(content.contains("\\t(0,70,"));
    let block = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "b.ass",
        &DynamicSubtitlesOptions {
            style: SubtitleStyle::Block,
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap();
    let content = std::fs::read_to_string(rt.runtime.workspace.existing("b.ass").unwrap()).unwrap();
    assert_eq!(block.groups, content.matches("Dialogue:").count());
    assert!(content.contains("\\fad(80,80)"));
}

#[test]
fn test_create_dynamic_subtitles_invalid() {
    let rt = runtime();
    let defaults = DynamicSubtitlesOptions::default();
    let error = create_dynamic_subtitles(&rt.runtime, &[], "a.ass", &defaults).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = create_dynamic_subtitles(&rt.runtime, &words(), "a.srt", &defaults).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "a.ass",
        &DynamicSubtitlesOptions {
            max_words: 0,
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "a.ass",
        &DynamicSubtitlesOptions {
            highlight_color: "ouro".to_string(),
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let all = words();
    let unordered = vec![all[1].clone(), all[0].clone()];
    let error = create_dynamic_subtitles(&rt.runtime, &unordered, "a.ass", &defaults).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_create_dynamic_subtitles_missing_video() {
    let rt = runtime();
    let error = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "a.ass",
        &DynamicSubtitlesOptions {
            video_path: Some("ghost.mp4".to_string()),
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_dynamic_subtitles_burn() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let subs = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "din.ass",
        &DynamicSubtitlesOptions {
            video_path: Some(sample.clone()),
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap();
    assert_eq!(subs.play_res, "320x240");
    let result = unwrap(burn_subtitles(
        &rt.runtime,
        &sample,
        &subs.output,
        BurnOptions::default(),
        false,
    ));
    assert!(result.styled_by_file);
    assert_eq!(result.output, "sample_subtitled.mp4");
}
