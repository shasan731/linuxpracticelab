//! Fixture builders for the catalogue and progression tests.

use std::path::Path;

pub fn lesson_json(module: &str, id: &str, prerequisites: &[&str]) -> String {
    let prerequisites = prerequisites
        .iter()
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"{{
  "schemaVersion": 1,
  "id": "{id}",
  "title": "Fixture lesson {id}",
  "level": "beginner",
  "module": "{module}",
  "type": "guided-practice",
  "estimatedDifficulty": 1,
  "prerequisites": [{prerequisites}],
  "concepts": ["directories"],
  "commands": ["mkdir"],
  "environment": {{
    "profile": "filesystem-basic",
    "resetPolicy": "per-attempt",
    "networkMode": "disabled",
    "sudoAllowed": false
  }},
  "content": {{
    "purpose": "Fixture purpose text for tests.",
    "mentalModel": "Fixture mental model text for tests."
  }},
  "tasks": [
    {{
      "id": "task-1",
      "kind": "guided",
      "instruction": "Create the reports directory in your home directory.",
      "validators": [
        {{ "type": "directory_exists", "path": "/home/student/reports" }}
      ],
      "hints": ["You need a command that creates directories."],
      "suggestedSolution": "mkdir reports"
    }}
  ]
}}
"#
    )
}

pub fn module_json(id: &str, number: u32, lessons: &[&str], pack: &str) -> String {
    let lessons = lessons
        .iter()
        .map(|l| format!("\"{l}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"{{
  "schemaVersion": 1,
  "id": "{id}",
  "number": {number},
  "title": "Fixture module {id}",
  "level": "beginner",
  "summary": "A fixture module used by the automated tests only.",
  "pack": "{pack}",
  "lessons": [{lessons}]
}}
"#
    )
}

pub fn write_module(root: &Path, pack: &str, id: &str, number: u32, lessons: &[&str]) {
    let dir = root.join(pack).join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("module.json"),
        module_json(id, number, lessons, pack),
    )
    .unwrap();
}

pub fn write_lesson(root: &Path, pack: &str, module: &str, id: &str, prerequisites: &[&str]) {
    let dir = root.join(pack).join(module);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{id}.json")),
        lesson_json(module, id, prerequisites),
    )
    .unwrap();
}
