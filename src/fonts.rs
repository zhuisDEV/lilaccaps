use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Result, bail};

const CJK_FONTS: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "C:\\Windows\\Fonts\\msyh.ttc",
];

const LATIN_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/HelveticaNeue.ttc",
    "C:\\Windows\\Fonts\\arial.ttf",
];

/// Resolve a font family through Fontconfig, or a caller-supplied font file.
/// ImageMagick needs a usable font file, whereas libass can accept a family name.
pub fn resolve_font_path(requested: Option<&str>, sample_text: &str) -> Result<PathBuf> {
    let requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "auto");
    if let Some(requested) = requested {
        let path = Path::new(requested);
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        if looks_like_font_path(requested) {
            bail!("font file does not exist or is not a regular file: {requested}");
        }
        if let Some(path) = named_candidates(requested)
            .iter()
            .find(|path| Path::new(path).is_file())
        {
            return Ok(PathBuf::from(path));
        }
    }

    let language = if sample_text
        .chars()
        .any(|ch| matches!(ch as u32, 0x3040..=0x30ff | 0x31f0..=0x31ff))
    {
        "ja"
    } else if sample_text
        .chars()
        .any(|ch| matches!(ch as u32, 0xac00..=0xd7af))
    {
        "ko"
    } else if sample_text.chars().any(is_cjk_or_korean_or_japanese) {
        "zh-cn"
    } else {
        "en"
    };
    let pattern = format!("{}:lang={language}", requested.unwrap_or("sans-serif"));
    if let Ok(output) = Command::new("fc-match")
        .args(["--format", "%{file}\n", "--", &pattern])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        && output.status.success()
        && let Some(path) = String::from_utf8_lossy(&output.stdout).lines().next()
        && Path::new(path).is_file()
    {
        return Ok(PathBuf::from(path));
    }

    let candidates = if language == "en" {
        LATIN_FONTS
    } else {
        CJK_FONTS
    };
    if let Some(path) = candidates.iter().find(|path| Path::new(path).is_file()) {
        return Ok(PathBuf::from(path));
    }
    bail!(
        "no usable font found; install a font for language {language} (for example Noto Sans CJK for Chinese), or provide --font /absolute/path/to/font.ttf"
    )
}

fn looks_like_font_path(value: &str) -> bool {
    value.contains('/')
        || value.contains('\\')
        || Path::new(value).extension().is_some_and(|extension| {
            matches!(
                extension
                    .to_str()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            )
        })
}

fn named_candidates(name: &str) -> &'static [&'static str] {
    let normalized: String = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    match normalized.as_str() {
        "arial" => &[
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
        ],
        "verdana" => &[
            "/System/Library/Fonts/Supplemental/Verdana.ttf",
            "/Library/Fonts/Verdana.ttf",
            "C:\\Windows\\Fonts\\verdana.ttf",
        ],
        "pingfang" | "pingfangsc" => &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/AssetsV2/com_apple_MobileAsset_Font8/86ba2c91f017a3749571a82f2c6d890ac7ffb2fb.asset/AssetData/PingFang.ttc",
        ],
        "hiraginosans" | "hiraginosansgb" => &[
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        ],
        "helvetica" | "helveticaneue" => &[
            "/System/Library/Fonts/Helvetica.ttc",
            "/System/Library/Fonts/HelveticaNeue.ttc",
        ],
        _ => &[],
    }
}

pub fn is_cjk_or_korean_or_japanese(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0x3040..=0x309f | 0x30a0..=0x30ff | 0x31f0..=0x31ff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0xff66..=0xff9d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_explicit_files_instead_of_substituting() {
        let error =
            resolve_font_path(Some("/nonexistent/lilaccaps/font.ttf"), "Hello").unwrap_err();
        assert!(error.to_string().contains("font file does not exist"));
    }

    #[test]
    fn resolves_installed_font_families_and_auto_to_real_files() {
        for requested in [None, Some("sans-serif"), Some("Arial")] {
            assert!(resolve_font_path(requested, "Hello").unwrap().is_file());
        }
        assert!(resolve_font_path(None, "中文字幕").unwrap().is_file());
    }

    #[test]
    fn identifies_supported_scripts() {
        assert!(is_cjk_or_korean_or_japanese('中'));
        assert!(is_cjk_or_korean_or_japanese('あ'));
        assert!(is_cjk_or_korean_or_japanese('한'));
        assert!(!is_cjk_or_korean_or_japanese('A'));
    }
}
