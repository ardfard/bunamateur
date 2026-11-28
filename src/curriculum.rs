//! Curriculum catalog: ordered units, teaching prerequisites, and coverage checks.
//!
//! Source of truth is `content/curriculum.json` (embedded at compile time).

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const CURRICULUM_JSON: &str = include_str!("../content/curriculum.json");

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PointStatus {
    Existing,
    Planned,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PointKind {
    Point,
    Contrast,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CurriculumPoint {
    pub slug: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    pub status: PointStatus,
    pub level: String,
    pub kind: PointKind,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Unit {
    pub id: String,
    pub n: i32,
    pub level: String,
    pub title: String,
    pub goal: String,
    pub sort_order: i32,
    pub points: Vec<CurriculumPoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SketchExistingPoint {
    pub slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: Option<PointStatus>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SketchLevel {
    pub level: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub themes: String,
    #[serde(default)]
    pub existing: Vec<SketchExistingPoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Curriculum {
    pub version: i32,
    pub units: Vec<Unit>,
    /// Optional N2/N1 sketch coverage for already-seeded points without full units yet.
    #[serde(default)]
    pub sketch: Vec<SketchLevel>,
}

/// Parse the embedded curriculum JSON.
pub fn load_curriculum() -> Result<Curriculum> {
    serde_json::from_str(CURRICULUM_JSON).context("Failed to parse embedded content/curriculum.json")
}

/// Parse curriculum JSON from an arbitrary string (tests / fixtures).
pub fn parse_curriculum(json: &str) -> Result<Curriculum> {
    serde_json::from_str(json).context("Failed to parse curriculum JSON")
}

impl Curriculum {
    /// Points in teaching order: units by `sort_order`, then points by `sort_order`.
    pub fn teaching_order(&self) -> Vec<&CurriculumPoint> {
        let mut units: Vec<&Unit> = self.units.iter().collect();
        units.sort_by_key(|u| u.sort_order);

        let mut out = Vec::new();
        for unit in units {
            let mut points: Vec<&CurriculumPoint> = unit.points.iter().collect();
            points.sort_by_key(|p| p.sort_order);
            out.extend(points);
        }
        out
    }

    /// Curriculum points with `status == existing` in teaching order (excludes sketch).
    pub fn existing_points(&self) -> Vec<&CurriculumPoint> {
        self.teaching_order()
            .into_iter()
            .filter(|p| p.status == PointStatus::Existing)
            .collect()
    }

    /// Slugs listed under `sketch[].existing` (coverage only; no unit placement).
    pub fn sketch_existing_slugs(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for level in &self.sketch {
            for point in &level.existing {
                out.push(point.slug.as_str());
            }
        }
        out
    }
}

/// Validate curriculum structure and coverage against the seeded grammar catalog.
///
/// Checks:
/// 1. All slugs unique across units (+ sketch.existing).
/// 2. `unit.id`, `unit.n`, and `unit.sort_order` are each unique across units.
/// 3. Across teaching order, `point.sort_order` values are unique and strictly increasing.
/// 4. Every prerequisite exists in teaching order and appears earlier (no cycles).
/// 5. Contrast entries have `kind == contrast` and earlier prerequisites (members taught first).
/// 6. Every `status == existing` slug is in `seeded_slugs`.
/// 7. No `status == planned` slug collides with `seeded_slugs`.
/// 8. Every seeded slug appears exactly once among Existing teaching-order points or sketch.existing.
pub fn validate_curriculum(
    curriculum: &Curriculum,
    seeded_slugs: &HashSet<&str>,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let order = curriculum.teaching_order();

    let mut seen_unit_ids: HashSet<&str> = HashSet::new();
    let mut seen_unit_ns: HashSet<i32> = HashSet::new();
    let mut seen_unit_sorts: HashSet<i32> = HashSet::new();
    for unit in &curriculum.units {
        if !seen_unit_ids.insert(unit.id.as_str()) {
            errors.push(format!("duplicate unit id: {}", unit.id));
        }
        if !seen_unit_ns.insert(unit.n) {
            errors.push(format!("duplicate unit n: {}", unit.n));
        }
        if !seen_unit_sorts.insert(unit.sort_order) {
            errors.push(format!("duplicate unit sort_order: {}", unit.sort_order));
        }
    }

    let mut position: HashMap<&str, usize> = HashMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut prev_point_sort: Option<i32> = None;
    let mut seen_point_sorts: HashSet<i32> = HashSet::new();

    for (idx, point) in order.iter().enumerate() {
        if !seen.insert(point.slug.as_str()) {
            errors.push(format!("duplicate slug in curriculum units: {}", point.slug));
        }
        position.insert(point.slug.as_str(), idx);

        if !seen_point_sorts.insert(point.sort_order) {
            errors.push(format!(
                "duplicate point sort_order {} for '{}'",
                point.sort_order, point.slug
            ));
        }
        if let Some(prev) = prev_point_sort
            && point.sort_order <= prev
        {
            errors.push(format!(
                "point sort_order for '{}' ({}) is not strictly greater than previous ({})",
                point.slug, point.sort_order, prev
            ));
        }
        prev_point_sort = Some(point.sort_order);
    }

    for slug in curriculum.sketch_existing_slugs() {
        if !seen.insert(slug) {
            errors.push(format!(
                "duplicate slug across units/sketch.existing: {slug}"
            ));
        }
    }

    for point in &order {
        for pre in &point.prerequisites {
            match position.get(pre.as_str()) {
                None => errors.push(format!(
                    "missing prerequisite '{pre}' for '{}'",
                    point.slug
                )),
                Some(&pre_idx) => {
                    let dep_idx = position[point.slug.as_str()];
                    if pre_idx >= dep_idx {
                        errors.push(format!(
                            "prerequisite '{pre}' for '{}' does not appear earlier in teaching order",
                            point.slug
                        ));
                    }
                }
            }
        }

        if point.kind == PointKind::Contrast {
            if point.prerequisites.is_empty() {
                errors.push(format!(
                    "contrast '{}' must list member points as prerequisites",
                    point.slug
                ));
            }
            // Members (prerequisites) must already be taught — covered by earlier-check above.
        } else if point.slug.starts_with("contrast-") && point.kind != PointKind::Contrast {
            errors.push(format!(
                "slug '{}' looks like a contrast but kind is not contrast",
                point.slug
            ));
        }
    }

    for point in &order {
        if point.status == PointStatus::Existing && !seeded_slugs.contains(point.slug.as_str()) {
            errors.push(format!(
                "existing curriculum slug '{}' is not in seeded_slugs",
                point.slug
            ));
        }
        if point.status == PointStatus::Planned && seeded_slugs.contains(point.slug.as_str()) {
            errors.push(format!(
                "planned curriculum slug '{}' collides with seeded_slugs",
                point.slug
            ));
        }
    }
    for slug in curriculum.sketch_existing_slugs() {
        if !seeded_slugs.contains(slug) {
            errors.push(format!(
                "sketch.existing slug '{slug}' is not in seeded_slugs"
            ));
        }
    }

    // Coverage counts only Existing teaching-order points + sketch.existing (not planned).
    let mut coverage: HashMap<&str, usize> = HashMap::new();
    for point in &order {
        if point.status == PointStatus::Existing {
            *coverage.entry(point.slug.as_str()).or_insert(0) += 1;
        }
    }
    for slug in curriculum.sketch_existing_slugs() {
        *coverage.entry(slug).or_insert(0) += 1;
    }

    for seed in seeded_slugs {
        match coverage.get(seed).copied().unwrap_or(0) {
            0 => errors.push(format!(
                "seeded slug '{seed}' is missing from curriculum units and sketch.existing"
            )),
            1 => {}
            n => errors.push(format!(
                "seeded slug '{seed}' appears {n} times in curriculum (expected exactly once)"
            )),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Convenience: load embedded curriculum and validate against a seeded slug set.
pub fn load_and_validate_curriculum(seeded_slugs: &HashSet<&str>) -> Result<Curriculum> {
    let curriculum = load_curriculum()?;
    if let Err(errors) = validate_curriculum(&curriculum, seeded_slugs) {
        bail!(
            "Curriculum validation failed ({} error(s)):\n{}",
            errors.len(),
            errors.join("\n")
        );
    }
    Ok(curriculum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed_data_extra::all_seed_grammar;

    fn seeded_from_catalog() -> HashSet<&'static str> {
        all_seed_grammar().into_iter().map(|g| g.slug).collect()
    }

    #[test]
    fn real_curriculum_loads_and_passes_validation() {
        let curriculum = load_curriculum().expect("load curriculum.json");
        assert_eq!(curriculum.version, 1);
        assert_eq!(curriculum.units.len(), 25);
        assert!(!curriculum.sketch.is_empty());

        let order = curriculum.teaching_order();
        assert_eq!(order.len(), 213);

        let seeded = seeded_from_catalog();
        assert_eq!(seeded.len(), 62);

        validate_curriculum(&curriculum, &seeded).unwrap_or_else(|errs| {
            panic!("real curriculum should pass validation:\n{}", errs.join("\n"));
        });

        let existing = curriculum.existing_points();
        assert_eq!(existing.len(), 56);
        assert_eq!(curriculum.sketch_existing_slugs().len(), 6);
        assert_eq!(existing.len() + curriculum.sketch_existing_slugs().len(), 62);
    }

    fn minimal_valid_json() -> &'static str {
        r#"{
          "version": 1,
          "units": [
            {
              "id": "u01",
              "n": 1,
              "level": "N5",
              "title": "Basics",
              "goal": "Start",
              "sort_order": 1,
              "points": [
                {
                  "slug": "a",
                  "title": "A",
                  "summary": "first",
                  "prerequisites": [],
                  "status": "existing",
                  "level": "N5",
                  "kind": "point",
                  "sort_order": 1
                },
                {
                  "slug": "b",
                  "title": "B",
                  "summary": "second",
                  "prerequisites": ["a"],
                  "status": "existing",
                  "level": "N5",
                  "kind": "point",
                  "sort_order": 2
                },
                {
                  "slug": "contrast-a-b",
                  "title": "A vs B",
                  "summary": "contrast",
                  "prerequisites": ["a", "b"],
                  "status": "planned",
                  "level": "N5",
                  "kind": "contrast",
                  "sort_order": 3
                }
              ]
            }
          ],
          "sketch": [
            {
              "level": "N2",
              "existing": [
                { "slug": "c", "title": "C", "status": "existing" }
              ]
            }
          ]
        }"#
    }

    #[test]
    fn minimal_fixture_passes() {
        let curriculum = parse_curriculum(minimal_valid_json()).unwrap();
        let seeded: HashSet<&str> = ["a", "b", "c"].into_iter().collect();
        validate_curriculum(&curriculum, &seeded).unwrap();
    }

    #[test]
    fn fails_on_duplicate_slug() {
        let json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"a","title":"A","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":1},
                {"slug":"a","title":"A2","summary":"","prerequisites":[],"status":"planned","level":"N5","kind":"point","sort_order":2}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(json).unwrap();
        let seeded: HashSet<&str> = ["a"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("duplicate")));
    }

    #[test]
    fn fails_on_missing_prerequisite() {
        let json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"a","title":"A","summary":"","prerequisites":["missing"],"status":"existing","level":"N5","kind":"point","sort_order":1}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(json).unwrap();
        let seeded: HashSet<&str> = ["a"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("missing prerequisite")));
    }

    #[test]
    fn fails_when_prerequisite_appears_after_dependent() {
        let json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"b","title":"B","summary":"","prerequisites":["a"],"status":"existing","level":"N5","kind":"point","sort_order":1},
                {"slug":"a","title":"A","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":2}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(json).unwrap();
        let seeded: HashSet<&str> = ["a", "b"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("does not appear earlier")));
    }

    #[test]
    fn fails_when_contrast_appears_before_members() {
        let json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"contrast-a-b","title":"A vs B","summary":"","prerequisites":["a","b"],"status":"planned","level":"N5","kind":"contrast","sort_order":1},
                {"slug":"a","title":"A","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":2},
                {"slug":"b","title":"B","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":3}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(json).unwrap();
        let seeded: HashSet<&str> = ["a", "b"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("does not appear earlier")),
            "expected earlier-order errors, got: {errs:?}"
        );
    }

    #[test]
    fn fails_when_existing_not_seeded() {
        let json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"ghost","title":"G","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":1}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(json).unwrap();
        let seeded: HashSet<&str> = HashSet::new();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("not in seeded_slugs")));
    }

    #[test]
    fn fails_when_seeded_not_in_curriculum() {
        let curriculum = parse_curriculum(minimal_valid_json()).unwrap();
        let seeded: HashSet<&str> = ["a", "b", "c", "orphan"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("orphan") && e.contains("missing")));
    }

    #[test]
    fn fails_when_planned_slug_is_also_seeded() {
        let json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"a","title":"A","summary":"","prerequisites":[],"status":"planned","level":"N5","kind":"point","sort_order":1}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(json).unwrap();
        let seeded: HashSet<&str> = ["a"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("collides with seeded_slugs")),
            "expected planned/seed collision, got: {errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("missing from curriculum")),
            "expected seeded slug missing from Existing coverage, got: {errs:?}"
        );
    }

    #[test]
    fn fails_on_duplicate_unit_id_n_or_sort_order() {
        let json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T1", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"a","title":"A","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":1}
              ]
            },
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T2", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"b","title":"B","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":2}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(json).unwrap();
        let seeded: HashSet<&str> = ["a", "b"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("duplicate unit id")));
        assert!(errs.iter().any(|e| e.contains("duplicate unit n")));
        assert!(errs.iter().any(|e| e.contains("duplicate unit sort_order")));
    }

    #[test]
    fn fails_on_non_increasing_or_duplicate_point_sort_order() {
        let duplicate_json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"a","title":"A","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":1},
                {"slug":"b","title":"B","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":1}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(duplicate_json).unwrap();
        let seeded: HashSet<&str> = ["a", "b"].into_iter().collect();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("duplicate point sort_order")),
            "expected duplicate point sort_order, got: {errs:?}"
        );

        // Cross-unit: teaching_order yields sort_order 5 then 3 → not strictly increasing.
        let cross_unit_json = r#"{
          "version": 1,
          "units": [
            {
              "id": "u01", "n": 1, "level": "N5", "title": "T1", "goal": "G", "sort_order": 1,
              "points": [
                {"slug":"a","title":"A","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":5}
              ]
            },
            {
              "id": "u02", "n": 2, "level": "N5", "title": "T2", "goal": "G", "sort_order": 2,
              "points": [
                {"slug":"b","title":"B","summary":"","prerequisites":[],"status":"existing","level":"N5","kind":"point","sort_order":3}
              ]
            }
          ]
        }"#;
        let curriculum = parse_curriculum(cross_unit_json).unwrap();
        let errs = validate_curriculum(&curriculum, &seeded).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("not strictly greater")),
            "expected non-increasing point sort_order, got: {errs:?}"
        );
    }
}
