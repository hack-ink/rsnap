use std::{
	collections::HashSet,
	sync::{Arc, OnceLock},
};

use egui::{FontData, FontDefinitions, FontFamily};
use fontdb::{Database, FaceInfo, Family, ID, Query, Stretch, Style, Weight};
use ttf_parser::Face;

type UnicodeCoverage = Vec<UnicodeCoverageRange>;

const NORMAL_WEIGHT_MIN: u16 = 300;
const NORMAL_WEIGHT_MAX: u16 = 700;
const MAX_SYSTEM_TEXT_FALLBACKS: usize = 16;
const MAX_SYSTEM_TEXT_COVERAGE_MISS_STREAK: usize = 32;

#[derive(Debug)]
pub(crate) struct SystemTextFont {
	name: String,
	font_data: Arc<FontData>,
}
impl SystemTextFont {
	pub(crate) fn egui_name(&self) -> &str {
		self.name.as_str()
	}

	pub(crate) fn egui_font_data(&self) -> Arc<FontData> {
		Arc::clone(&self.font_data)
	}
}

#[derive(Clone, Debug)]
struct CandidateSystemTextFont {
	face_id: ID,
	family_name: String,
	order: usize,
	weight_delta: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UnicodeCoverageRange {
	start: u32,
	end: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemTextFontProbeResult {
	Selected,
	SkippedNoCoverage,
	Skipped,
}

pub(crate) fn system_text_fonts() -> &'static [SystemTextFont] {
	static FONTS: OnceLock<Vec<SystemTextFont>> = OnceLock::new();

	FONTS.get_or_init(load_system_text_fonts).as_slice()
}

pub(crate) fn configure_text_font_fallbacks(fonts: &mut FontDefinitions) {
	for font in system_text_fonts() {
		fonts.font_data.insert(font.egui_name().to_owned(), font.egui_font_data());
	}

	let proportional_family = fonts.families.entry(FontFamily::Proportional).or_default();

	for font in system_text_fonts() {
		if !proportional_family.iter().any(|name| name == font.egui_name()) {
			proportional_family.push(font.egui_name().to_owned());
		}
	}
}

fn load_system_text_fonts() -> Vec<SystemTextFont> {
	let mut database = Database::new();

	database.load_system_fonts();

	let generic_sans_face_id = database.query(&Query {
		families: &[Family::SansSerif],
		weight: Weight::NORMAL,
		stretch: Stretch::Normal,
		style: Style::Normal,
	});
	let mut candidates = database
		.faces()
		.enumerate()
		.filter_map(|(order, face)| build_candidate_system_text_font(&database, face.id, order))
		.collect::<Vec<_>>();

	if let Some(face_id) = generic_sans_face_id
		&& !candidates.iter().any(|candidate| candidate.face_id == face_id)
		&& let Some(candidate) =
			build_candidate_system_text_font(&database, face_id, candidates.len())
	{
		candidates.push(candidate);
	}

	candidates.sort_by(|left, right| {
		left.weight_delta.cmp(&right.weight_delta).then_with(|| left.order.cmp(&right.order))
	});

	select_system_text_fonts(&database, &candidates, generic_sans_face_id)
}

fn build_candidate_system_text_font(
	database: &Database,
	face_id: ID,
	order: usize,
) -> Option<CandidateSystemTextFont> {
	let face = database.face(face_id)?;

	if !is_system_text_candidate_face(face) {
		return None;
	}

	Some(CandidateSystemTextFont {
		face_id,
		family_name: primary_family_name(face)?,
		order,
		weight_delta: face.weight.0.abs_diff(Weight::NORMAL.0),
	})
}

fn is_system_text_candidate_face(face: &FaceInfo) -> bool {
	!face.monospaced
		&& face.style == Style::Normal
		&& face.stretch == Stretch::Normal
		&& (NORMAL_WEIGHT_MIN..=NORMAL_WEIGHT_MAX).contains(&face.weight.0)
}

fn select_system_text_fonts(
	database: &Database,
	candidates: &[CandidateSystemTextFont],
	generic_sans_face_id: Option<ID>,
) -> Vec<SystemTextFont> {
	let mut selected = Vec::new();
	let mut selected_face_ids = HashSet::new();
	let mut selected_families = HashSet::new();
	let mut covered_codepoints = UnicodeCoverage::new();
	let mut consecutive_coverage_misses = 0_usize;

	if let Some(face_id) = generic_sans_face_id
		&& let Some(candidate) = candidates.iter().find(|candidate| candidate.face_id == face_id)
	{
		consecutive_coverage_misses = next_system_text_probe_miss_streak(
			consecutive_coverage_misses,
			try_select_system_text_font(
				database,
				candidate,
				&mut selected,
				&mut selected_face_ids,
				&mut selected_families,
				&mut covered_codepoints,
			),
		);
	}

	for candidate in candidates {
		if selected.len() >= MAX_SYSTEM_TEXT_FALLBACKS
			|| system_text_probe_miss_streak_exhausted(consecutive_coverage_misses)
		{
			break;
		}

		consecutive_coverage_misses = next_system_text_probe_miss_streak(
			consecutive_coverage_misses,
			try_select_system_text_font(
				database,
				candidate,
				&mut selected,
				&mut selected_face_ids,
				&mut selected_families,
				&mut covered_codepoints,
			),
		);
	}

	selected
}

fn next_system_text_probe_miss_streak(
	consecutive_coverage_misses: usize,
	probe_result: SystemTextFontProbeResult,
) -> usize {
	match probe_result {
		SystemTextFontProbeResult::Selected => 0,
		SystemTextFontProbeResult::SkippedNoCoverage => {
			consecutive_coverage_misses.saturating_add(1)
		},
		SystemTextFontProbeResult::Skipped => consecutive_coverage_misses,
	}
}

fn system_text_probe_miss_streak_exhausted(consecutive_coverage_misses: usize) -> bool {
	consecutive_coverage_misses >= MAX_SYSTEM_TEXT_COVERAGE_MISS_STREAK
}

fn try_select_system_text_font(
	database: &Database,
	candidate: &CandidateSystemTextFont,
	selected: &mut Vec<SystemTextFont>,
	selected_face_ids: &mut HashSet<ID>,
	selected_families: &mut HashSet<String>,
	covered_codepoints: &mut UnicodeCoverage,
) -> SystemTextFontProbeResult {
	if selected_face_ids.contains(&candidate.face_id)
		|| selected_families.contains(candidate.family_name.as_str())
		|| selected.len() >= MAX_SYSTEM_TEXT_FALLBACKS
	{
		return SystemTextFontProbeResult::Skipped;
	}

	let Some(coverage_codepoints) = load_system_text_coverage(database, candidate.face_id) else {
		return SystemTextFontProbeResult::Skipped;
	};

	if !selected.is_empty()
		&& !coverage_adds_new_codepoints(covered_codepoints, &coverage_codepoints)
	{
		return SystemTextFontProbeResult::SkippedNoCoverage;
	}

	let Some(font) = build_system_text_font(database, candidate.face_id) else {
		return SystemTextFontProbeResult::Skipped;
	};

	selected.push(font);
	selected_face_ids.insert(candidate.face_id);
	selected_families.insert(candidate.family_name.clone());

	merge_coverage_codepoints(covered_codepoints, &coverage_codepoints);

	SystemTextFontProbeResult::Selected
}

fn load_system_text_coverage(database: &Database, face_id: ID) -> Option<UnicodeCoverage> {
	database
		.with_face_data(face_id, |font_bytes, face_index| {
			let face = Face::parse(font_bytes, face_index).ok()?;
			let cmap = face.tables().cmap?;
			let mut code_points = Vec::new();

			for subtable in cmap.subtables {
				if !subtable.is_unicode() {
					continue;
				}

				subtable.codepoints(|code_point| code_points.push(code_point));
			}

			compress_codepoints_into_coverage(code_points)
		})
		.flatten()
}

fn primary_family_name(face: &FaceInfo) -> Option<String> {
	Some(face.families.first()?.0.clone())
}

fn build_system_text_font(database: &Database, face_id: ID) -> Option<SystemTextFont> {
	database
		.with_face_data(face_id, |font_bytes, face_index| {
			let mut font_data = FontData::from_owned(font_bytes.to_vec());

			font_data.index = face_index;

			Some(SystemTextFont {
				name: format!("system-fallback-{face_id}"),
				font_data: Arc::new(font_data),
			})
		})
		.flatten()
}

fn compress_codepoints_into_coverage(mut code_points: Vec<u32>) -> Option<UnicodeCoverage> {
	if code_points.is_empty() {
		return None;
	}

	code_points.sort_unstable();
	code_points.dedup();

	let mut coverage = Vec::new();
	let mut start = code_points[0];
	let mut end = code_points[0];

	for code_point in code_points.into_iter().skip(1) {
		if code_point <= end.saturating_add(1) {
			end = code_point;

			continue;
		}

		push_coverage_range(&mut coverage, UnicodeCoverageRange { start, end });

		start = code_point;
		end = code_point;
	}

	push_coverage_range(&mut coverage, UnicodeCoverageRange { start, end });

	Some(coverage)
}

fn push_coverage_range(coverage: &mut UnicodeCoverage, range: UnicodeCoverageRange) {
	if let Some(last_range) = coverage.last_mut()
		&& range.start <= last_range.end.saturating_add(1)
	{
		last_range.end = last_range.end.max(range.end);

		return;
	}

	coverage.push(range);
}

fn coverage_adds_new_codepoints(
	covered_codepoints: &UnicodeCoverage,
	candidate_codepoints: &UnicodeCoverage,
) -> bool {
	let mut covered_index = 0;

	for candidate_range in candidate_codepoints {
		while covered_index < covered_codepoints.len()
			&& covered_codepoints[covered_index].end < candidate_range.start
		{
			covered_index += 1;
		}

		let mut uncovered_start = candidate_range.start;
		let mut scan_index = covered_index;

		while scan_index < covered_codepoints.len()
			&& covered_codepoints[scan_index].start <= candidate_range.end
		{
			let covered_range = covered_codepoints[scan_index];

			if covered_range.start > uncovered_start {
				return true;
			}

			uncovered_start = covered_range.end.saturating_add(1);

			if uncovered_start > candidate_range.end {
				break;
			}

			scan_index += 1;
		}

		if uncovered_start <= candidate_range.end {
			return true;
		}
	}

	false
}

fn merge_coverage_codepoints(
	covered_codepoints: &mut UnicodeCoverage,
	candidate_codepoints: &UnicodeCoverage,
) {
	let mut merged = Vec::with_capacity(covered_codepoints.len() + candidate_codepoints.len());
	let mut covered_index = 0;
	let mut candidate_index = 0;

	while covered_index < covered_codepoints.len() || candidate_index < candidate_codepoints.len() {
		let next_range = match (
			covered_codepoints.get(covered_index).copied(),
			candidate_codepoints.get(candidate_index).copied(),
		) {
			(Some(covered_range), Some(candidate_range))
				if covered_range.start <= candidate_range.start =>
			{
				covered_index += 1;

				covered_range
			},
			(Some(_), Some(candidate_range)) => {
				candidate_index += 1;

				candidate_range
			},
			(Some(covered_range), None) => {
				covered_index += 1;

				covered_range
			},
			(None, Some(candidate_range)) => {
				candidate_index += 1;

				candidate_range
			},
			(None, None) => break,
		};

		push_coverage_range(&mut merged, next_range);
	}

	*covered_codepoints = merged;
}

#[cfg(test)]
mod tests {
	use crate::system_fonts::{SystemTextFontProbeResult, UnicodeCoverage, UnicodeCoverageRange};

	#[test]
	fn coverage_tracks_arbitrary_script_codepoints() {
		let mut coverage = UnicodeCoverage::new();

		super::push_coverage_range(
			&mut coverage,
			UnicodeCoverageRange { start: u32::from('ᚠ'), end: u32::from('ᚠ') },
		);
		super::push_coverage_range(
			&mut coverage,
			UnicodeCoverageRange { start: u32::from('𐓐'), end: u32::from('𐓐') },
		);

		assert_eq!(
			coverage,
			vec![
				UnicodeCoverageRange { start: u32::from('ᚠ'), end: u32::from('ᚠ') },
				UnicodeCoverageRange { start: u32::from('𐓐'), end: u32::from('𐓐') },
			]
		);
	}

	#[test]
	fn coverage_adds_new_codepoints_within_same_unicode_page() {
		let candidate_codepoints =
			vec![UnicodeCoverageRange { start: u32::from('z'), end: u32::from('z') }];
		let mut covered_codepoints =
			vec![UnicodeCoverageRange { start: u32::from('A'), end: u32::from('A') }];

		assert!(super::coverage_adds_new_codepoints(&covered_codepoints, &candidate_codepoints,));

		super::merge_coverage_codepoints(&mut covered_codepoints, &candidate_codepoints);

		assert_eq!(
			covered_codepoints,
			vec![
				UnicodeCoverageRange { start: u32::from('A'), end: u32::from('A') },
				UnicodeCoverageRange { start: u32::from('z'), end: u32::from('z') }
			]
		);
		assert!(!super::coverage_adds_new_codepoints(&covered_codepoints, &candidate_codepoints,));
	}

	#[test]
	fn system_text_probe_miss_streak_resets_after_selected_font() {
		let misses =
			super::next_system_text_probe_miss_streak(7, SystemTextFontProbeResult::Selected);

		assert_eq!(misses, 0);
	}

	#[test]
	fn system_text_probe_miss_streak_advances_on_no_coverage_fonts() {
		let misses = super::next_system_text_probe_miss_streak(
			3,
			SystemTextFontProbeResult::SkippedNoCoverage,
		);

		assert_eq!(misses, 4);
		assert!(super::system_text_probe_miss_streak_exhausted(
			super::MAX_SYSTEM_TEXT_COVERAGE_MISS_STREAK
		));
	}
}
