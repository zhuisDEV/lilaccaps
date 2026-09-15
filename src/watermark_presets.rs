//! Portable watermark definitions with owned image/font assets.
//!
//! A new directory reserves a name without replacing existing data. The manifest
//! is published last with an exclusive hard link, so readers only use complete
//! presets. Deletion inspects the owned layout and never follows symlinks.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::runtime::ScopedTempPath;
use crate::watermark::{WatermarkSource, WatermarkStyle, normalized_opacity};

const FORMAT_VERSION: u32 = 1;
const MARKER: &str = ".lilaccaps-watermark";
const MARKER_CONTENTS: &str = "lilaccaps-watermark-v1\n";
const MANIFEST: &str = "preset.json";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_ASSET_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct SavedWatermark {
    pub name: String,
    pub format_version: u32,
    pub directory: PathBuf,
    pub source: WatermarkSource,
    pub style: WatermarkStyle,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format_version: u32,
    name: String,
    source: StoredSource,
    style: WatermarkStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_asset: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum StoredSource {
    Text { text: String },
    Image { asset: String },
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        bail!(
            "watermark name must be 1-64 lowercase ASCII letters, digits, hyphens or underscores, starting with a letter or digit"
        );
    }
    Ok(())
}

pub fn save(
    runtime_home: &Path,
    name: &str,
    source: &WatermarkSource,
    style: &WatermarkStyle,
) -> Result<SavedWatermark> {
    validate_name(name)?;
    let root = runtime_home.join("watermarks");
    checked_directory(&root, true)?;
    save_directory(&root.join(name), name, source, style)
}

pub fn load(runtime_home: &Path, name: &str) -> Result<SavedWatermark> {
    validate_name(name)?;
    load_directory(&runtime_home.join("watermarks").join(name), Some(name))
}

pub fn list(runtime_home: &Path) -> Result<Vec<SavedWatermark>> {
    let root = runtime_home.join("watermarks");
    if !checked_directory(&root, false)? {
        return Ok(Vec::new());
    }
    let mut presets = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .context("watermark preset name is not UTF-8")?;
        validate_name(name)?;
        checked_directory(&entry.path(), false)?;
        // A save in progress has reserved the name but has not published its
        // manifest. Such a directory is deliberately not a usable preset yet.
        if fs::symlink_metadata(entry.path().join(MANIFEST))
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        presets.push(load_directory(&entry.path(), Some(name))?);
    }
    presets.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(presets)
}

pub fn remove(runtime_home: &Path, name: &str) -> Result<()> {
    validate_name(name)?;
    let directory = runtime_home.join("watermarks").join(name);
    // Loading checks the version, source paths, ownership marker, and exact
    // directory layout before any file is removed.
    load_directory(&directory, Some(name))?;
    remove_owned_directory(&directory)
}

/// Make a job-owned copy that survives removal or replacement of a global preset.
pub fn snapshot(preset: &SavedWatermark, destination: &Path) -> Result<SavedWatermark> {
    save_directory(destination, &preset.name, &preset.source, &preset.style)
}

pub fn load_snapshot(destination: &Path) -> Result<SavedWatermark> {
    load_directory(destination, None)
}

fn save_directory(
    directory: &Path,
    name: &str,
    source: &WatermarkSource,
    style: &WatermarkStyle,
) -> Result<SavedWatermark> {
    validate_name(name)?;
    validate_style(style)?;
    let source = normalise_source(source)?;
    let parent = directory
        .parent()
        .context("preset directory has no parent")?;
    checked_directory(parent, true)?;
    fs::create_dir(directory).with_context(|| {
        format!(
            "cannot create watermark preset {}; names and snapshot directories must be new",
            directory.display()
        )
    })?;
    let mut created = Vec::new();
    let result = (|| -> Result<()> {
        write_new(
            &directory.join(MARKER),
            MARKER_CONTENTS.as_bytes(),
            &mut created,
        )?;
        fs::create_dir(directory.join("assets"))?;
        let stored_source = match &source {
            WatermarkSource::Text(text) => StoredSource::Text { text: text.clone() },
            WatermarkSource::Image(image) => {
                let extension = image_extension(image)?;
                let asset = format!("assets/image.{extension}");
                copy_asset(image, &directory.join(&asset), &mut created)?;
                StoredSource::Image { asset }
            }
        };
        let mut stored_style = style.clone();
        stored_style.colour = stored_style.colour.trim().to_string();
        stored_style.outline_colour = stored_style.outline_colour.trim().to_string();
        let mut font_asset = None;
        if let Some(font) = style.font.as_deref() {
            if looks_like_font_path(font) || Path::new(font).exists() {
                let extension = font_extension(Path::new(font))?;
                let asset = format!("assets/font.{extension}");
                copy_asset(Path::new(font), &directory.join(&asset), &mut created)?;
                stored_style.font = None;
                font_asset = Some(asset);
            } else {
                stored_style.font = Some(font.trim().to_string());
            }
        }
        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            name: name.to_string(),
            source: stored_source,
            style: stored_style,
            font_asset,
        };
        let json = serde_json::to_vec_pretty(&manifest)?;
        let temporary = ScopedTempPath::file(directory, "preset", Some("json"));
        write_new(temporary.path(), &json, &mut created)?;
        fs::hard_link(temporary.path(), directory.join(MANIFEST))
            .context("failed to publish watermark preset manifest without overwriting")?;
        created.push(directory.join(MANIFEST));
        Ok(())
    })();
    if let Err(error) = result {
        // Only remove files this invocation created. Unknown files prevent
        // directory removal; no recursive deletion can escape the preset.
        for path in created.iter().rev() {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_dir(directory.join("assets"));
        let _ = fs::remove_dir(directory);
        return Err(error);
    }
    load_directory(directory, Some(name))
}

fn load_directory(directory: &Path, expected_name: Option<&str>) -> Result<SavedWatermark> {
    if !checked_directory(directory, false)? {
        bail!("watermark preset does not exist: {}", directory.display());
    }
    validate_marker(directory)?;
    let manifest_path = directory.join(MANIFEST);
    checked_regular_file(&manifest_path, MAX_MANIFEST_BYTES)?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)
        .with_context(|| format!("invalid watermark preset {}", manifest_path.display()))?;
    if manifest.format_version != FORMAT_VERSION {
        bail!(
            "unsupported watermark preset format version {}",
            manifest.format_version
        );
    }
    validate_name(&manifest.name)?;
    if expected_name.is_some_and(|name| name != manifest.name) {
        bail!("watermark preset name does not match its directory");
    }
    if manifest
        .style
        .font
        .as_deref()
        .is_some_and(looks_like_font_path)
    {
        bail!("watermark preset font files must be owned assets");
    }
    validate_style(&manifest.style)?;
    let mut expected_assets = BTreeSet::new();
    let source = match manifest.source {
        StoredSource::Text { text } => normalise_source(&WatermarkSource::Text(text))?,
        StoredSource::Image { asset } => {
            let path = owned_asset(directory, &asset, "image")?;
            expected_assets.insert(asset_filename(&asset)?.to_string());
            WatermarkSource::Image(path)
        }
    };
    let mut style = manifest.style;
    if let Some(asset) = manifest.font_asset {
        if style.font.is_some() {
            bail!("watermark preset cannot contain both a font family and a font asset");
        }
        let path = owned_asset(directory, &asset, "font")?;
        expected_assets.insert(asset_filename(&asset)?.to_string());
        style.font = Some(path.to_string_lossy().into_owned());
    } else if style.font.as_deref().is_some_and(looks_like_font_path) {
        bail!("watermark preset font files must be owned assets");
    }
    validate_layout(directory, &expected_assets)?;
    Ok(SavedWatermark {
        name: manifest.name,
        format_version: manifest.format_version,
        directory: directory.to_path_buf(),
        source,
        style,
    })
}

fn normalise_source(source: &WatermarkSource) -> Result<WatermarkSource> {
    match source {
        WatermarkSource::Text(text) => {
            let text = text.trim();
            if text.is_empty() || text.len() > 16 * 1024 || text.contains('\0') {
                bail!(
                    "text watermark must be non-empty, contain no NUL, and be at most 16384 bytes"
                );
            }
            Ok(WatermarkSource::Text(text.to_string()))
        }
        WatermarkSource::Image(path) => {
            checked_regular_file(path, MAX_ASSET_BYTES)?;
            validate_image(path)?;
            Ok(WatermarkSource::Image(path.clone()))
        }
    }
}

pub fn validate_style(style: &WatermarkStyle) -> Result<()> {
    normalized_opacity(style.opacity)?;
    if style.size > 16384 || style.margin > 16384 || style.outline_width > 1024 {
        bail!("watermark size and margin must be at most 16384; outline width at most 1024");
    }
    for (label, colour) in [
        ("colour", &style.colour),
        ("outline colour", &style.outline_colour),
    ] {
        if !valid_colour(colour.trim()) {
            bail!("watermark {label} must be a named FFmpeg colour or #RRGGBB/#RRGGBBAA");
        }
    }
    if let Some(font) = &style.font {
        if font.trim().is_empty() || font.chars().any(char::is_control) {
            bail!("watermark font must be a non-empty font family or font file path");
        }
        if looks_like_font_path(font) || Path::new(font).exists() {
            checked_regular_file(Path::new(font), MAX_ASSET_BYTES)?;
            font_extension(Path::new(font))?;
            validate_font_file(Path::new(font))?;
        }
    }
    Ok(())
}

fn valid_colour(colour: &str) -> bool {
    if let Some(hex) = colour
        .strip_prefix('#')
        .or_else(|| colour.strip_prefix("0x"))
    {
        return matches!(hex.len(), 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    // FFmpeg's named colour set. Keeping validation local also lets text presets
    // be managed before the rendering dependencies have been installed.
    const NAMES: &str = "AliceBlue AntiqueWhite Aqua Aquamarine Azure Beige Bisque Black \
        BlanchedAlmond Blue BlueViolet Brown BurlyWood CadetBlue Chartreuse Chocolate Coral \
        CornflowerBlue Cornsilk Crimson Cyan DarkBlue DarkCyan DarkGoldenRod DarkGray DarkGreen \
        DarkKhaki DarkMagenta DarkOliveGreen DarkOrange DarkOrchid DarkRed DarkSalmon DarkSeaGreen \
        DarkSlateBlue DarkSlateGray DarkTurquoise DarkViolet DeepPink DeepSkyBlue DimGray DodgerBlue \
        FireBrick FloralWhite ForestGreen Fuchsia Gainsboro GhostWhite Gold GoldenRod Gray Green \
        GreenYellow HoneyDew HotPink IndianRed Indigo Ivory Khaki Lavender LavenderBlush LawnGreen \
        LemonChiffon LightBlue LightCoral LightCyan LightGoldenRodYellow LightGreen LightGrey \
        LightPink LightSalmon LightSeaGreen LightSkyBlue LightSlateGray LightSteelBlue LightYellow \
        Lime LimeGreen Linen Magenta Maroon MediumAquaMarine MediumBlue MediumOrchid MediumPurple \
        MediumSeaGreen MediumSlateBlue MediumSpringGreen MediumTurquoise MediumVioletRed MidnightBlue \
        MintCream MistyRose Moccasin NavajoWhite Navy OldLace Olive OliveDrab Orange OrangeRed Orchid \
        PaleGoldenRod PaleGreen PaleTurquoise PaleVioletRed PapayaWhip PeachPuff Peru Pink Plum \
        PowderBlue Purple Red RosyBrown RoyalBlue SaddleBrown Salmon SandyBrown SeaGreen SeaShell \
        Sienna Silver SkyBlue SlateBlue SlateGray Snow SpringGreen SteelBlue Tan Teal Thistle Tomato \
        Turquoise Violet Wheat White WhiteSmoke Yellow YellowGreen";
    NAMES
        .split_ascii_whitespace()
        .any(|name| name.eq_ignore_ascii_case(colour))
}

fn looks_like_font_path(value: &str) -> bool {
    value.contains('/')
        || value.contains('\\')
        || value.starts_with('.')
        || Path::new(value)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ["ttf", "ttc", "otf", "otc"]
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            })
}

fn font_extension(path: &Path) -> Result<String> {
    let extension = extension(path)?;
    if !matches!(extension.as_str(), "ttf" | "ttc" | "otf" | "otc") {
        bail!("watermark font asset must be a TTF, TTC, OTF or OTC file");
    }
    Ok(extension)
}

fn validate_font_file(path: &Path) -> Result<()> {
    let mut signature = [0; 4];
    File::open(path)?
        .read_exact(&mut signature)
        .context("watermark font file has no valid font header")?;
    if !matches!(
        &signature,
        b"\0\x01\0\0" | b"OTTO" | b"ttcf" | b"true" | b"typ1"
    ) {
        bail!(
            "watermark font file has an unsupported font header: {}",
            path.display()
        );
    }
    Ok(())
}

fn image_extension(path: &Path) -> Result<String> {
    let extension = extension(path)?;
    if !matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "svg"
            | "svgz"
            | "ppm"
            | "pgm"
            | "pbm"
            | "pnm"
    ) {
        bail!("unsupported watermark image extension: {extension}");
    }
    Ok(extension)
}

fn extension(path: &Path) -> Result<String> {
    Ok(path
        .extension()
        .and_then(|value| value.to_str())
        .context("watermark asset must have a supported file extension")?
        .to_ascii_lowercase())
}

fn validate_image(path: &Path) -> Result<()> {
    let extension = image_extension(path)?;
    let absolute = fs::canonicalize(path)?;
    if matches!(extension.as_str(), "svg" | "svgz") {
        let output = Command::new("magick")
            .args(["identify", "-ping", "-format", "%w %h"])
            .arg(&absolute)
            .output()
            .context("ImageMagick is required to validate SVG watermark assets")?;
        let dimensions = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .take(2)
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()?;
        if !output.status.success() || dimensions.len() != 2 || dimensions.contains(&0) {
            bail!("invalid SVG watermark image: {}", path.display());
        }
    } else {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height",
                "-of",
                "json",
            ])
            .arg(&absolute)
            .output()
            .context("ffprobe is required to validate image watermark assets")?;
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("ffprobe returned invalid image metadata")?;
        let stream = &value["streams"][0];
        let image_codec = stream["codec_name"].as_str().is_some_and(|codec| {
            matches!(
                codec,
                "png" | "mjpeg" | "webp" | "gif" | "bmp" | "tiff" | "ppm" | "pgm" | "pbm" | "pam"
            )
        });
        if !output.status.success()
            || !output.stderr.is_empty()
            || !image_codec
            || stream["width"].as_u64().unwrap_or(0) == 0
            || stream["height"].as_u64().unwrap_or(0) == 0
        {
            bail!("invalid watermark image: {}", path.display());
        }
    }
    Ok(())
}

fn copy_asset(source: &Path, destination: &Path, created: &mut Vec<PathBuf>) -> Result<()> {
    checked_regular_file(source, MAX_ASSET_BYTES)?;
    let mut input = File::open(source)?.take(MAX_ASSET_BYTES + 1);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    created.push(destination.to_path_buf());
    if std::io::copy(&mut input, &mut output)? > MAX_ASSET_BYTES {
        bail!("watermark asset grew beyond its size limit while copying");
    }
    output.sync_all()?;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8], created: &mut Vec<PathBuf>) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    created.push(path.to_path_buf());
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn checked_directory(path: &Path, create: bool) -> Result<bool> {
    if !path.is_absolute() {
        bail!(
            "watermark storage path must be absolute: {}",
            path.display()
        );
    }
    let mut current = PathBuf::new();
    let mut missing = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                current.push(component)
            }
            Component::CurDir | Component::ParentDir => bail!(
                "watermark storage path must be normalised: {}",
                path.display()
            ),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_symlink() || !metadata.is_dir() => {
                bail!(
                    "watermark storage cannot follow a symlink or non-directory: {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                if create {
                    match fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            let metadata = fs::symlink_metadata(&current)?;
                            if metadata.is_symlink() || !metadata.is_dir() {
                                bail!(
                                    "watermark storage path is not a real directory: {}",
                                    current.display()
                                );
                            }
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(create || !missing)
}

fn checked_regular_file(path: &Path, max_bytes: u64) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot read watermark asset {}", path.display()))?;
    if metadata.is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > max_bytes
    {
        bail!(
            "watermark asset must be a non-empty regular file of at most {max_bytes} bytes, without symlinks: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_marker(directory: &Path) -> Result<()> {
    let marker = directory.join(MARKER);
    checked_regular_file(&marker, 128)?;
    if fs::read_to_string(&marker)? != MARKER_CONTENTS {
        bail!(
            "directory is not an owned LilacCaps watermark preset: {}",
            directory.display()
        );
    }
    Ok(())
}

fn asset_filename(asset: &str) -> Result<&str> {
    let filename = asset
        .strip_prefix("assets/")
        .context("watermark asset must be inside assets/")?;
    if filename.is_empty() || filename.contains(['/', '\\']) || filename.contains("..") {
        bail!("invalid watermark asset path: {asset}");
    }
    Ok(filename)
}

fn owned_asset(directory: &Path, asset: &str, kind: &str) -> Result<PathBuf> {
    let filename = asset_filename(asset)?;
    if !filename.starts_with(&format!("{kind}.")) {
        bail!("watermark {kind} asset has an unexpected filename");
    }
    checked_directory(&directory.join("assets"), false)?;
    let path = directory.join("assets").join(filename);
    checked_regular_file(&path, MAX_ASSET_BYTES)?;
    if kind == "image" {
        image_extension(&path)?;
    } else {
        font_extension(&path)?;
        validate_font_file(&path)?;
    }
    Ok(path)
}

fn validate_layout(directory: &Path, expected_assets: &BTreeSet<String>) -> Result<()> {
    let expected = [MARKER, MANIFEST, "assets"]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if entry_names(directory)? != expected {
        bail!(
            "watermark preset contains unexpected files: {}",
            directory.display()
        );
    }
    checked_directory(&directory.join("assets"), false)?;
    if entry_names(&directory.join("assets"))? != *expected_assets {
        bail!("watermark preset assets do not match its manifest");
    }
    Ok(())
}

fn entry_names(directory: &Path) -> Result<BTreeSet<String>> {
    fs::read_dir(directory)?
        .map(|entry| {
            let entry = entry?;
            entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("watermark preset filename is not UTF-8"))
        })
        .collect()
}

fn remove_owned_directory(directory: &Path) -> Result<()> {
    validate_marker(directory)?;
    let assets = directory.join("assets");
    checked_directory(&assets, false)?;
    for entry in fs::read_dir(&assets)? {
        let path = entry?.path();
        checked_regular_file(&path, MAX_ASSET_BYTES)?;
    }
    for entry in fs::read_dir(&assets)? {
        fs::remove_file(entry?.path())?;
    }
    fs::remove_dir(&assets)?;
    fs::remove_file(directory.join(MANIFEST))?;
    fs::remove_file(directory.join(MARKER))?;
    fs::remove_dir(directory)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watermark::WatermarkPosition;

    fn fixture() -> ScopedTempPath {
        ScopedTempPath::directory(
            &std::env::temp_dir().canonicalize().unwrap(),
            "watermark-presets-test",
        )
        .unwrap()
    }

    fn style() -> WatermarkStyle {
        WatermarkStyle {
            position: WatermarkPosition::TopRight,
            opacity: 0.7,
            size: 48,
            margin: 40,
            colour: "#ffd54f".into(),
            font: Some("DejaVu Sans".into()),
            outline_colour: "black".into(),
            outline_width: 2,
        }
    }

    fn text() -> WatermarkSource {
        WatermarkSource::Text("LILAC".into())
    }

    fn image_fixture(directory: &Path) -> PathBuf {
        let path = directory.join("original.ppm");
        let mut pixels = b"P6\n2 2\n255\n".to_vec();
        pixels.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        fs::write(&path, pixels).unwrap();
        path
    }

    #[test]
    fn text_preset_round_trips_every_style_and_lists_in_name_order() {
        let root = fixture();
        save(root.path(), "z-last", &text(), &style()).unwrap();
        save(root.path(), "lilac", &text(), &style()).unwrap();
        let saved = load(root.path(), "lilac").unwrap();
        assert_eq!(saved.format_version, FORMAT_VERSION);
        assert_eq!(saved.source.label(), "LILAC");
        assert_eq!(
            serde_json::to_value(saved.style).unwrap(),
            serde_json::to_value(style()).unwrap()
        );
        assert_eq!(
            list(root.path())
                .unwrap()
                .iter()
                .map(|preset| preset.name.as_str())
                .collect::<Vec<_>>(),
            ["lilac", "z-last"]
        );
        remove(root.path(), "lilac").unwrap();
        assert!(load(root.path(), "lilac").is_err());
        assert_eq!(list(root.path()).unwrap().len(), 1);
    }

    #[test]
    fn duplicate_names_and_empty_preexisting_directories_are_never_overwritten() {
        let root = fixture();
        let saved = save(root.path(), "lilac", &text(), &style()).unwrap();
        let original = fs::read(saved.directory.join(MANIFEST)).unwrap();
        assert!(
            save(
                root.path(),
                "lilac",
                &WatermarkSource::Text("changed".into()),
                &style()
            )
            .is_err()
        );
        assert_eq!(fs::read(saved.directory.join(MANIFEST)).unwrap(), original);
        fs::create_dir(root.path().join("watermarks/reserved")).unwrap();
        assert!(save(root.path(), "reserved", &text(), &style()).is_err());
        assert!(root.path().join("watermarks/reserved").is_dir());
    }

    #[test]
    fn concurrent_saves_publish_only_one_complete_preset() {
        let root = fixture();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads = (0..2)
            .map(|_| {
                let path = root.path().to_path_buf();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    save(&path, "lilac", &text(), &style()).is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            threads
                .into_iter()
                .map(|thread| usize::from(thread.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert_eq!(list(root.path()).unwrap().len(), 1);
    }

    #[test]
    fn image_and_snapshot_survive_removal_of_their_originals() {
        let root = fixture();
        let image = image_fixture(root.path());
        let bytes = fs::read(&image).unwrap();
        let preset = save(
            root.path(),
            "logo",
            &WatermarkSource::Image(image.clone()),
            &style(),
        )
        .unwrap();
        fs::remove_file(image).unwrap();
        let snapshot_path = root.path().join("job/watermark");
        snapshot(&preset, &snapshot_path).unwrap();
        remove(root.path(), "logo").unwrap();
        let saved = load_snapshot(&snapshot_path).unwrap();
        let WatermarkSource::Image(path) = saved.source else {
            panic!("expected image")
        };
        assert!(path.starts_with(snapshot_path));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn explicit_font_files_are_owned_assets_in_presets_and_snapshots() {
        let root = fixture();
        let original_font = crate::fonts::resolve_font_path(None, "LILAC").unwrap();
        let font_copy = root.path().join(format!(
            "external.{}",
            original_font.extension().unwrap().to_str().unwrap()
        ));
        fs::copy(&original_font, &font_copy).unwrap();
        let mut style = style();
        style.font = Some(font_copy.to_string_lossy().into_owned());
        let preset = save(root.path(), "lilac", &text(), &style).unwrap();
        fs::remove_file(font_copy).unwrap();
        let saved_font = PathBuf::from(preset.style.font.as_ref().unwrap());
        assert!(saved_font.starts_with(&preset.directory));
        assert_eq!(
            fs::read(saved_font).unwrap(),
            fs::read(original_font).unwrap()
        );
        let copied = snapshot(&preset, &root.path().join("job-watermark")).unwrap();
        remove(root.path(), "lilac").unwrap();
        assert!(Path::new(copied.style.font.as_ref().unwrap()).is_file());
    }

    #[test]
    fn invalid_names_sources_and_styles_never_publish_a_preset() {
        let root = fixture();
        for name in [
            "",
            ".",
            "..",
            "../outside",
            "a/b",
            "a\\b",
            "Lilac",
            "中文",
            "-lilac",
        ] {
            assert!(
                save(root.path(), name, &text(), &style()).is_err(),
                "{name}"
            );
        }
        for opacity in [f32::NAN, -0.1, 1.1] {
            let mut invalid = style();
            invalid.opacity = opacity;
            assert!(save(root.path(), "invalid", &text(), &invalid).is_err());
        }
        for colour in ["", "not-a-colour", "#123", "#中中", "white@0.2"] {
            let mut invalid = style();
            invalid.colour = colour.into();
            assert!(save(root.path(), "invalid", &text(), &invalid).is_err());
        }
        let mut invalid_font = style();
        invalid_font.font = Some(
            root.path()
                .join("missing.ttf")
                .to_string_lossy()
                .into_owned(),
        );
        assert!(save(root.path(), "invalid", &text(), &invalid_font).is_err());
        assert!(
            save(
                root.path(),
                "invalid",
                &WatermarkSource::Text("   ".into()),
                &style()
            )
            .is_err()
        );
        let corrupt = root.path().join("invalid.png");
        fs::write(&corrupt, "not an image").unwrap();
        assert!(
            save(
                root.path(),
                "invalid",
                &WatermarkSource::Image(corrupt),
                &style()
            )
            .is_err()
        );
        assert!(list(root.path()).unwrap().is_empty());
        assert!(!root.path().join("watermarks/invalid").exists());
    }

    #[test]
    fn tampered_manifest_cannot_reference_or_remove_external_files() {
        let root = fixture();
        let preset = save(root.path(), "lilac", &text(), &style()).unwrap();
        let outside = root.path().join("outside.png");
        fs::write(&outside, "keep me").unwrap();
        let manifest = preset.directory.join(MANIFEST);
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        for asset in [
            "../../outside.png",
            "assets/../../outside.png",
            "assets\\outside.png",
            "/tmp/outside.png",
        ] {
            let mut tampered = original.clone();
            tampered["source"] = serde_json::json!({"kind":"image", "asset":asset});
            fs::write(&manifest, serde_json::to_vec(&tampered).unwrap()).unwrap();
            assert!(load(root.path(), "lilac").is_err());
            assert!(remove(root.path(), "lilac").is_err());
            assert_eq!(fs::read_to_string(&outside).unwrap(), "keep me");
        }
    }

    #[test]
    fn removal_refuses_unknown_files_and_unknown_manifest_versions() {
        let root = fixture();
        let preset = save(root.path(), "lilac", &text(), &style()).unwrap();
        let foreign = preset.directory.join("notes.txt");
        fs::write(&foreign, "keep me").unwrap();
        assert!(remove(root.path(), "lilac").is_err());
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "keep me");
        fs::remove_file(foreign).unwrap();
        let manifest = preset.directory.join(MANIFEST);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["format_version"] = serde_json::json!(99);
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load(root.path(), "lilac").is_err());
        assert!(remove(root.path(), "lilac").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_storage_manifest_and_assets_are_rejected_without_touching_targets() {
        use std::os::unix::fs::symlink;
        let root = fixture();
        let outside = fixture();
        symlink(outside.path(), root.path().join("watermarks")).unwrap();
        assert!(save(root.path(), "lilac", &text(), &style()).is_err());
        assert!(list(root.path()).is_err());
        fs::remove_file(root.path().join("watermarks")).unwrap();
        let preset = save(root.path(), "lilac", &text(), &style()).unwrap();
        let manifest = preset.directory.join(MANIFEST);
        let external_manifest = outside.path().join(MANIFEST);
        fs::rename(&manifest, &external_manifest).unwrap();
        symlink(&external_manifest, &manifest).unwrap();
        assert!(load(root.path(), "lilac").is_err());
        assert!(remove(root.path(), "lilac").is_err());
        assert!(external_manifest.is_file());
        fs::remove_file(&manifest).unwrap();
        fs::rename(&external_manifest, &manifest).unwrap();
        fs::remove_dir(preset.directory.join("assets")).unwrap();
        symlink(outside.path(), preset.directory.join("assets")).unwrap();
        assert!(load(root.path(), "lilac").is_err());
        assert!(remove(root.path(), "lilac").is_err());
        assert!(outside.path().is_dir());
    }
}
