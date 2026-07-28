//! Loads and indexes the lesson catalogue from disk.
//!
//! The catalogue is the union of `lessons/core` and `lessons/optional`. Loading is strict:
//! a malformed package, an unknown validator or a prerequisite cycle is reported rather
//! than skipped, because a lesson that silently fails to load looks to the learner like a
//! curriculum with a hole in it.

use anyhow::{anyhow, bail, Context, Result};
use shared_types::{registry, Lesson, Module, ResetPolicy};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Catalog {
    lessons: BTreeMap<String, Lesson>,
    modules: BTreeMap<String, Module>,
    /// Module ids in teaching order.
    module_order: Vec<String>,
    /// Flattened lesson ids in teaching order across all core modules.
    lesson_order: Vec<String>,
    /// Problems that did not stop loading but should be surfaced to a lesson author.
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogIssue {
    pub source: String,
    pub message: String,
}

impl std::fmt::Display for CatalogIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.source, self.message)
    }
}

impl Catalog {
    /// Loads every module manifest under `root`, then the lessons each one lists.
    pub fn load(root: &Path) -> Result<Self> {
        let mut catalog = Catalog::default();
        let mut modules: Vec<(PathBuf, Module)> = Vec::new();

        for pack in ["core", "optional"] {
            let pack_dir = root.join(pack);
            if !pack_dir.is_dir() {
                continue;
            }
            let entries = std::fs::read_dir(&pack_dir)
                .with_context(|| format!("could not read {}", pack_dir.display()))?;
            for entry in entries.flatten() {
                let manifest = entry.path().join("module.json");
                if !manifest.is_file() {
                    continue;
                }
                let module: Module = read_json(&manifest)?;
                modules.push((entry.path(), module));
            }
        }

        if modules.is_empty() {
            bail!(
                "no lesson modules found under {}. The installation may be incomplete.",
                root.display()
            );
        }

        modules.sort_by_key(|(_, m)| (m.pack != "core", m.number));

        for (dir, module) in modules {
            for lesson_id in &module.lessons {
                let path = dir.join(format!("{lesson_id}.json"));
                let lesson: Lesson = read_json(&path)
                    .with_context(|| format!("module {} lists lesson {lesson_id}", module.id))?;
                if lesson.id != *lesson_id {
                    bail!(
                        "{} declares id {} but is listed as {lesson_id}",
                        path.display(),
                        lesson.id
                    );
                }
                if lesson.module != module.id {
                    bail!(
                        "lesson {} says it belongs to module {} but is listed by {}",
                        lesson.id,
                        lesson.module,
                        module.id
                    );
                }
                if catalog.lessons.contains_key(lesson_id) {
                    bail!("duplicate lesson id {lesson_id}");
                }
                if module.pack == "core" {
                    catalog.lesson_order.push(lesson_id.clone());
                }
                catalog.lessons.insert(lesson_id.clone(), lesson);
            }
            catalog.module_order.push(module.id.clone());
            catalog.modules.insert(module.id.clone(), module);
        }

        // Every integrity issue is fatal. A lesson that silently fails to load looks to the
        // learner like a curriculum with a hole in it, which is harder to diagnose than a
        // refusal to start, so nothing here is downgraded to a warning.
        let issues = catalog.integrity_issues();
        if !issues.is_empty() {
            let rendered = issues
                .iter()
                .map(|issue| issue.to_string())
                .collect::<Vec<_>>()
                .join("\n  ");
            bail!("the lesson catalogue is not consistent:\n  {rendered}");
        }

        // Warnings are for things that do not stop the catalogue loading but are worth telling
        // a lesson author about; they surface in the UI.
        catalog.warnings = catalog.advisory_notes();

        Ok(catalog)
    }

    /// Checks referential integrity: prerequisites resolve, no cycles, validators are known.
    /// Used both at load time and by the authoring CLI.
    pub fn integrity_issues(&self) -> Vec<CatalogIssue> {
        let mut issues = Vec::new();

        for (id, lesson) in &self.lessons {
            for prerequisite in &lesson.prerequisites {
                if !self.lessons.contains_key(prerequisite) {
                    issues.push(CatalogIssue {
                        source: id.clone(),
                        message: format!("prerequisite {prerequisite} does not exist"),
                    });
                }
                if prerequisite == id {
                    issues.push(CatalogIssue {
                        source: id.clone(),
                        message: "lesson lists itself as a prerequisite".into(),
                    });
                }
            }

            for task in &lesson.tasks {
                for validator in &task.validators {
                    if let Err(err) = registry().check(validator) {
                        issues.push(CatalogIssue {
                            source: format!("{id}/{}", task.id),
                            message: err.to_string(),
                        });
                    }
                }
            }

            let mut seen_task_ids = HashSet::new();
            for task in &lesson.tasks {
                if !seen_task_ids.insert(&task.id) {
                    issues.push(CatalogIssue {
                        source: id.clone(),
                        message: format!("duplicate task id {}", task.id),
                    });
                }
            }
        }

        for cycle in self.prerequisite_cycles() {
            issues.push(CatalogIssue {
                source: cycle.first().cloned().unwrap_or_default(),
                message: format!("prerequisite cycle: {}", cycle.join(" -> ")),
            });
        }

        for (module_id, module) in &self.modules {
            if module.lessons.is_empty() {
                issues.push(CatalogIssue {
                    source: module_id.clone(),
                    message: "module lists no lessons".into(),
                });
            }
        }

        issues
    }

    /// Non-fatal observations for lesson authors, shown in the UI rather than blocking startup.
    fn advisory_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        for (id, lesson) in &self.lessons {
            // A per-attempt lesson that creates state in setup but has no reset script cannot
            // actually restore itself, so a second attempt starts dirty.
            if lesson.environment.reset_policy == ResetPolicy::PerAttempt
                && lesson.environment.setup_script.is_some()
                && lesson.environment.reset_script.is_none()
            {
                notes.push(format!(
                    "{id} resets per attempt and has a setup script but no reset script, so \
                     Reset task cannot restore what setup created"
                ));
            }
            if lesson.commands.is_empty() {
                notes.push(format!(
                    "{id} lists no commands, so it will not appear in the command reference"
                ));
            }
        }
        notes
    }

    /// Depth-first search reporting each cycle once.
    fn prerequisite_cycles(&self) -> Vec<Vec<String>> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Open,
            Done,
        }

        let mut marks: HashMap<&str, Mark> = HashMap::new();
        let mut cycles = Vec::new();
        let mut stack: Vec<&str> = Vec::new();

        fn visit<'a>(
            id: &'a str,
            lessons: &'a BTreeMap<String, Lesson>,
            marks: &mut HashMap<&'a str, Mark>,
            stack: &mut Vec<&'a str>,
            cycles: &mut Vec<Vec<String>>,
        ) {
            match marks.get(id) {
                Some(Mark::Done) => return,
                Some(Mark::Open) => {
                    // Found a back edge; report the loop from where it started.
                    if let Some(start) = stack.iter().position(|s| *s == id) {
                        let mut cycle: Vec<String> =
                            stack[start..].iter().map(|s| s.to_string()).collect();
                        cycle.push(id.to_string());
                        cycles.push(cycle);
                    }
                    return;
                }
                None => {}
            }
            marks.insert(id, Mark::Open);
            stack.push(id);
            if let Some(lesson) = lessons.get(id) {
                for prerequisite in &lesson.prerequisites {
                    if lessons.contains_key(prerequisite) {
                        visit(prerequisite, lessons, marks, stack, cycles);
                    }
                }
            }
            stack.pop();
            marks.insert(id, Mark::Done);
        }

        for id in self.lessons.keys() {
            visit(id, &self.lessons, &mut marks, &mut stack, &mut cycles);
        }
        cycles
    }

    pub fn lesson(&self, id: &str) -> Option<&Lesson> {
        self.lessons.get(id)
    }

    pub fn try_lesson(&self, id: &str) -> Result<&Lesson> {
        self.lessons
            .get(id)
            .ok_or_else(|| anyhow!("no lesson with id {id}"))
    }

    pub fn module(&self, id: &str) -> Option<&Module> {
        self.modules.get(id)
    }

    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.module_order
            .iter()
            .filter_map(move |id| self.modules.get(id))
    }

    pub fn lessons(&self) -> impl Iterator<Item = &Lesson> {
        self.lesson_order
            .iter()
            .filter_map(move |id| self.lessons.get(id))
    }

    pub fn core_lesson_ids(&self) -> &[String] {
        &self.lesson_order
    }

    pub fn lesson_count(&self) -> usize {
        self.lessons.len()
    }

    pub fn core_lesson_count(&self) -> usize {
        self.lesson_order.len()
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Every command the curriculum touches, for the Command Reference cross-links.
    pub fn all_commands(&self) -> BTreeSet<&str> {
        self.lessons
            .values()
            .flat_map(|l| l.commands.iter().map(|c| c.as_str()))
            .collect()
    }

    /// Lessons that introduce or exercise a command.
    pub fn lessons_for_command(&self, command: &str) -> Vec<&Lesson> {
        self.lesson_order
            .iter()
            .filter_map(|id| self.lessons.get(id))
            .filter(|l| l.commands.iter().any(|c| c == command))
            .collect()
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("could not parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{write_lesson, write_module};

    #[test]
    fn loads_modules_in_teaching_order_with_optional_packs_last() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_module(root, "core", "second", 1, &["second.01"]);
        write_lesson(root, "core", "second", "second.01", &[]);
        write_module(root, "core", "first", 0, &["first.01"]);
        write_lesson(root, "core", "first", "first.01", &[]);
        write_module(root, "optional", "extra", 0, &["extra.01"]);
        write_lesson(root, "optional", "extra", "extra.01", &[]);

        let catalog = Catalog::load(root).unwrap();
        let order: Vec<&str> = catalog.modules().map(|m| m.id.as_str()).collect();
        assert_eq!(order, vec!["first", "second", "extra"]);

        // Optional packs do not extend the core guided path.
        assert_eq!(catalog.core_lesson_ids(), &["first.01", "second.01"]);
        assert_eq!(catalog.lesson_count(), 3);
    }

    #[test]
    fn a_missing_prerequisite_stops_the_load() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_module(root, "core", "m", 0, &["m.01"]);
        write_lesson(root, "core", "m", "m.01", &["m.99"]);

        let err = Catalog::load(root).unwrap_err().to_string();
        assert!(err.contains("m.99 does not exist"), "{err}");
    }

    #[test]
    fn a_prerequisite_cycle_stops_the_load() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_module(root, "core", "m", 0, &["m.01", "m.02"]);
        write_lesson(root, "core", "m", "m.01", &["m.02"]);
        write_lesson(root, "core", "m", "m.02", &["m.01"]);

        let err = Catalog::load(root).unwrap_err().to_string();
        assert!(err.contains("prerequisite cycle"), "{err}");
    }

    #[test]
    fn an_unknown_validator_stops_the_load() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_module(root, "core", "m", 0, &["m.01"]);
        let lesson = crate::test_support::lesson_json("m", "m.01", &[]).replace(
            r#""type": "directory_exists""#,
            r#""type": "directory_exists_typo""#,
        );
        std::fs::write(root.join("core/m/m.01.json"), lesson).unwrap();

        let err = Catalog::load(root).unwrap_err().to_string();
        assert!(err.contains("unknown validator"), "{err}");
    }

    #[test]
    fn an_id_that_disagrees_with_its_filename_stops_the_load() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_module(root, "core", "m", 0, &["m.01"]);
        let lesson = crate::test_support::lesson_json("m", "m.99", &[]);
        std::fs::write(root.join("core/m/m.01.json"), lesson).unwrap();

        let err = Catalog::load(root).unwrap_err().to_string();
        assert!(err.contains("declares id m.99"), "{err}");
    }

    #[test]
    fn an_empty_lessons_root_is_reported_as_an_incomplete_installation() {
        let dir = tempfile::tempdir().unwrap();
        let err = Catalog::load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("installation may be incomplete"), "{err}");
    }

    #[test]
    fn commands_are_indexed_for_the_reference() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_module(root, "core", "m", 0, &["m.01"]);
        write_lesson(root, "core", "m", "m.01", &[]);
        let catalog = Catalog::load(root).unwrap();
        assert!(catalog.all_commands().contains("mkdir"));
        assert_eq!(catalog.lessons_for_command("mkdir").len(), 1);
        assert_eq!(catalog.lessons_for_command("nftables").len(), 0);
    }
}
