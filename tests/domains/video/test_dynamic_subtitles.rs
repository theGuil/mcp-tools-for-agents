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

#[test]
fn test_create_dynamic_subtitles_karaoke_and_box() {
    let rt = runtime();
    let karaoke = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "k.ass",
        &DynamicSubtitlesOptions {
            style: SubtitleStyle::Karaoke,
            max_gap: 0.5,
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap();
    let content = std::fs::read_to_string(rt.runtime.workspace.existing("k.ass").unwrap()).unwrap();
    // Um evento por bloco, com \kf por palavra.
    assert_eq!(karaoke.groups, content.matches("Dialogue:").count());
    assert_eq!(content.matches("\\kf").count(), 6);
    assert!(content.contains("{\\kf40}OLÁ"));
    let boxed = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "b.ass",
        &DynamicSubtitlesOptions {
            style: SubtitleStyle::Box,
            highlight_color: "#7C3AED".to_string(),
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap();
    let content = std::fs::read_to_string(rt.runtime.workspace.existing("b.ass").unwrap()).unwrap();
    assert_eq!(boxed.words, 6);
    assert!(content.contains("\\3c&H00ED3A7C&"));
    assert!(content.contains("{\\r}"));
}

#[test]
fn test_create_dynamic_subtitles_presets() {
    use mcp_tools::domains::video::dynamic_subtitles::SubtitlePreset;
    let rt = runtime();
    let presets = [
        SubtitlePreset::Hormozi,
        SubtitlePreset::Boxed,
        SubtitlePreset::Karaoke,
        SubtitlePreset::Pop,
        SubtitlePreset::Clean,
        SubtitlePreset::Neon,
    ];
    for preset in presets {
        let options = DynamicSubtitlesOptions::preset(preset);
        let output = format!("{preset:?}.ass").to_lowercase();
        let result = create_dynamic_subtitles(&rt.runtime, &words(), &output, &options).unwrap();
        assert_eq!(result.preset, Some(preset));
        assert_eq!(result.style, options.style);
        let content =
            std::fs::read_to_string(rt.runtime.workspace.existing(&output).unwrap()).unwrap();
        assert!(content.contains("[Events]"));
        assert!(content.contains("Style: Default,DejaVu Sans,"));
    }
    // Hormozi: 3 palavras por bloco, maiúsculas, fonte maior que o padrão.
    let hormozi = DynamicSubtitlesOptions::preset(SubtitlePreset::Hormozi);
    let result = create_dynamic_subtitles(&rt.runtime, &words(), "h.ass", &hormozi).unwrap();
    assert!(result.font_size > (1920.0 * 0.045) as i64);
    let content = std::fs::read_to_string(rt.runtime.workspace.existing("h.ass").unwrap()).unwrap();
    assert!(content.contains("OLÁ"));
    // Clean: sem maiúsculas, sem negrito, no rodapé (alinhamento 2).
    let clean = DynamicSubtitlesOptions::preset(SubtitlePreset::Clean);
    create_dynamic_subtitles(&rt.runtime, &words(), "c.ass", &clean).unwrap();
    let content = std::fs::read_to_string(rt.runtime.workspace.existing("c.ass").unwrap()).unwrap();
    assert!(content.contains("olá"));
    assert!(content.contains(",0,0,0,0,100,100,0,0,1,"));
    assert!(content.contains(",2,64,64,307,1\n"));
}

#[test]
fn test_create_dynamic_subtitles_invalid_style_values() {
    let rt = runtime();
    let error = create_dynamic_subtitles(
        &rt.runtime,
        &words(),
        "a.ass",
        &DynamicSubtitlesOptions {
            outline: Some(-1.0),
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
            font_name: "  ".to_string(),
            ..DynamicSubtitlesOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_dynamic_subtitles_presets_burn() {
    use mcp_tools::domains::video::dynamic_subtitles::SubtitlePreset;
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    for preset in [SubtitlePreset::Boxed, SubtitlePreset::Karaoke] {
        let options = DynamicSubtitlesOptions {
            video_path: Some(sample.clone()),
            ..DynamicSubtitlesOptions::preset(preset)
        };
        let output = format!("{preset:?}.ass").to_lowercase();
        let subs = create_dynamic_subtitles(&rt.runtime, &words(), &output, &options).unwrap();
        let result = unwrap(burn_subtitles(
            &rt.runtime,
            &sample,
            &subs.output,
            BurnOptions::default(),
            false,
        ));
        assert!(result.styled_by_file);
    }
}
