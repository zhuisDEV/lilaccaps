use std::ops::Range;

const ANALYSIS_FRAME_MS: usize = 20;

#[derive(Debug, Clone, Copy)]
pub struct SpeechWindowOptions {
    pub min_speech_ms: usize,
    pub min_silence_ms: usize,
    pub padding_ms: usize,
    pub max_window_seconds: usize,
    pub overlap_seconds: usize,
}

#[derive(Debug, Clone)]
pub struct SpeechWindowPlan {
    pub windows: Vec<Range<usize>>,
    pub threshold: f32,
    pub speech_region_count: usize,
    pub speech_sample_count: usize,
}

pub fn fixed_windows(
    sample_count: usize,
    sample_rate: usize,
    window_seconds: usize,
    overlap_seconds: usize,
) -> Vec<Range<usize>> {
    let window_size = sample_rate.saturating_mul(window_seconds).max(1);
    let overlap_size = sample_rate
        .saturating_mul(overlap_seconds)
        .min(window_size.saturating_sub(1));
    split_range(0..sample_count, window_size, overlap_size)
}

pub fn speech_windows(
    audio: &[f32],
    sample_rate: usize,
    options: SpeechWindowOptions,
) -> SpeechWindowPlan {
    if audio.is_empty() || sample_rate == 0 {
        return SpeechWindowPlan {
            windows: Vec::new(),
            threshold: 0.0,
            speech_region_count: 0,
            speech_sample_count: 0,
        };
    }

    let frame_size = milliseconds_to_samples(ANALYSIS_FRAME_MS, sample_rate).max(1);
    let frame_levels = audio
        .chunks(frame_size)
        .map(root_mean_square)
        .collect::<Vec<_>>();
    let threshold = adaptive_speech_threshold(&frame_levels);
    let min_silence_samples =
        milliseconds_to_samples(options.min_silence_ms, sample_rate).max(frame_size);
    let min_speech_samples =
        milliseconds_to_samples(options.min_speech_ms, sample_rate).max(frame_size);

    let raw_regions = active_frame_regions(&frame_levels, threshold, frame_size, audio.len());
    let merged_regions = merge_nearby_regions(raw_regions, min_silence_samples);
    let speech_regions = merged_regions
        .into_iter()
        .filter(|region| region.len() >= min_speech_samples)
        .collect::<Vec<_>>();
    let speech_sample_count = speech_regions.iter().map(Range::len).sum();
    let speech_region_count = speech_regions.len();

    let padding_samples = milliseconds_to_samples(options.padding_ms, sample_rate);
    let padded_regions = speech_regions
        .into_iter()
        .map(|region| {
            region.start.saturating_sub(padding_samples)
                ..region.end.saturating_add(padding_samples).min(audio.len())
        })
        .collect::<Vec<_>>();
    let padded_regions = merge_nearby_regions(padded_regions, 0);

    let max_window_samples = sample_rate
        .saturating_mul(options.max_window_seconds)
        .max(1);
    let overlap_samples = sample_rate
        .saturating_mul(options.overlap_seconds)
        .min(max_window_samples.saturating_sub(1));
    let windows = padded_regions
        .into_iter()
        .flat_map(|region| split_range(region, max_window_samples, overlap_samples))
        .collect();

    SpeechWindowPlan {
        windows,
        threshold,
        speech_region_count,
        speech_sample_count,
    }
}

fn root_mean_square(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let square_sum = frame
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum::<f64>();
    (square_sum / frame.len() as f64).sqrt() as f32
}

fn adaptive_speech_threshold(levels: &[f32]) -> f32 {
    if levels.is_empty() {
        return 0.0;
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let noise_floor = percentile(&sorted, 20);
    let active_level = percentile(&sorted, 90);
    if active_level < 0.003 {
        return 0.003;
    }

    (noise_floor * 2.5)
        .max(active_level * 0.08)
        .max(0.003)
        .min(active_level * 0.5)
}

fn percentile(sorted: &[f32], percentile: usize) -> f32 {
    let index = sorted
        .len()
        .saturating_sub(1)
        .saturating_mul(percentile.min(100))
        / 100;
    sorted[index]
}

fn active_frame_regions(
    levels: &[f32],
    threshold: f32,
    frame_size: usize,
    sample_count: usize,
) -> Vec<Range<usize>> {
    let mut regions = Vec::new();
    let mut active_start = None;

    for (index, level) in levels.iter().enumerate() {
        if *level >= threshold {
            active_start.get_or_insert(index.saturating_mul(frame_size));
        } else if let Some(start) = active_start.take() {
            regions.push(start..index.saturating_mul(frame_size).min(sample_count));
        }
    }
    if let Some(start) = active_start {
        regions.push(start..sample_count);
    }
    regions
}

fn merge_nearby_regions(
    regions: Vec<Range<usize>>,
    maximum_gap_samples: usize,
) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::new();
    for region in regions {
        if let Some(previous) = merged.last_mut()
            && region.start.saturating_sub(previous.end) <= maximum_gap_samples
        {
            previous.end = previous.end.max(region.end);
            continue;
        }
        merged.push(region);
    }
    merged
}

fn split_range(
    region: Range<usize>,
    maximum_size: usize,
    overlap_size: usize,
) -> Vec<Range<usize>> {
    if region.is_empty() {
        return Vec::new();
    }

    let maximum_size = maximum_size.max(1);
    let overlap_size = overlap_size.min(maximum_size.saturating_sub(1));
    let step_size = maximum_size.saturating_sub(overlap_size).max(1);
    let mut windows = Vec::new();
    let mut start = region.start;

    while start < region.end {
        let end = region.end.min(start.saturating_add(maximum_size));
        windows.push(start..end);
        if end == region.end {
            break;
        }
        start = start.saturating_add(step_size);
    }
    windows
}

fn milliseconds_to_samples(milliseconds: usize, sample_rate: usize) -> usize {
    sample_rate.saturating_mul(milliseconds) / 1_000
}

#[cfg(test)]
mod tests {
    use super::{SpeechWindowOptions, fixed_windows, speech_windows};

    fn options() -> SpeechWindowOptions {
        SpeechWindowOptions {
            min_speech_ms: 400,
            min_silence_ms: 350,
            padding_ms: 300,
            max_window_seconds: 30,
            overlap_seconds: 2,
        }
    }

    #[test]
    fn fixed_windows_overlap_without_exceeding_input() {
        assert_eq!(fixed_windows(75, 1, 30, 2), vec![0..30, 28..58, 56..75]);
    }

    #[test]
    fn silence_produces_no_speech_windows() {
        let plan = speech_windows(&vec![0.0; 1_000], 100, options());
        assert!(plan.windows.is_empty());
        assert_eq!(plan.speech_region_count, 0);
    }

    #[test]
    fn speech_regions_are_filtered_padded_and_kept_separate() {
        let mut audio = vec![0.0; 1_000];
        audio[200..300].fill(0.1);
        audio[600..700].fill(0.1);
        let plan = speech_windows(&audio, 100, options());

        assert_eq!(plan.speech_region_count, 2);
        assert_eq!(plan.speech_sample_count, 200);
        assert_eq!(plan.windows, vec![170..330, 570..730]);
    }

    #[test]
    fn short_silence_is_bridged_before_padding() {
        let mut audio = vec![0.0; 500];
        audio[100..200].fill(0.1);
        audio[225..325].fill(0.1);
        let plan = speech_windows(&audio, 100, options());

        assert_eq!(plan.speech_region_count, 1);
        assert_eq!(plan.windows, vec![70..356]);
    }

    #[test]
    fn short_noise_burst_is_not_treated_as_speech() {
        let mut audio = vec![0.0; 500];
        audio[100..120].fill(0.1);
        let plan = speech_windows(&audio, 100, options());

        assert!(plan.windows.is_empty());
    }

    #[test]
    fn long_speech_region_uses_overlapping_bounded_windows() {
        let audio = vec![0.1; 800];
        let mut options = options();
        options.padding_ms = 0;
        options.max_window_seconds = 3;
        options.overlap_seconds = 1;
        let plan = speech_windows(&audio, 100, options);

        assert_eq!(plan.windows, vec![0..300, 200..500, 400..700, 600..800]);
    }
}
