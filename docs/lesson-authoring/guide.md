# Writing a lesson

A lesson is one JSON file. `npm run lessons:validate` is the gate, and it is deliberately strict:
everything it rejects is something that would otherwise reach a learner as a lesson that cannot be
passed, or one that can be passed without doing anything.

## The rule that shapes everything

**Check state, never commands.** If a task can be satisfied three different ways, all three must
pass. If you find yourself wanting to check *what was typed*, the task is asking the wrong thing.

```jsonc
// Right: the goal is a directory that exists.
{ "type": "directory_exists", "path": "/home/student/reports" }

// Wrong: there is no validator for this, on purpose.
{ "type": "command_was", "command": "mkdir reports" }
```

## Skeleton

```jsonc
{
  "schemaVersion": 1,
  "id": "filesystem.04",              // must equal the file name
  "title": "Relative Paths",
  "level": "beginner",
  "module": "filesystem-navigation",  // must match the module.json that lists it
  "type": "guided-practice",
  "estimatedDifficulty": 2,
  "prerequisites": ["filesystem.03"],
  "concepts": ["relative paths", "working directory"],
  "commands": ["cd", "pwd"],          // drives the command reference cross-links
  "environment": {
    "profile": "filesystem-basic",
    "resetPolicy": "per-attempt",
    "networkMode": "disabled",
    "sudoAllowed": false
  },
  "content": {
    "purpose": "One sentence: what problem this solves.",
    "mentalModel": "What Linux is actually doing. This is the part that makes it stick."
  },
  "tasks": [ /* … */ ],
  "reviewQuestions": [ /* … */ ]
}
```

Then add it to the module's `lessons` array. A lesson file that exists but is not listed is
invisible at run time, so the validator treats that as an error rather than letting it hide.

## Tasks

Four kinds, following the teaching pattern:

| Kind | What it does | Requires |
| --- | --- | --- |
| `guided` | Tells the learner exactly what to do | `suggestedSolution` |
| `independent` | States the goal only | `suggestedSolution` |
| `mistake` | Presents a broken command to diagnose | `brokenCommand`, `diagnosis` |
| `applied` | Puts it in a realistic scenario | `suggestedSolution` |
| `assessment` | No hints, no examples | — |

```jsonc
{
  "id": "task-1",
  "kind": "guided",
  "instruction": "Create a directory called reports in your home directory.",
  "validators": [
    { "type": "directory_exists", "path": "/home/student/reports" }
  ],
  "hints": [
    "You need a command that creates directories.",   // conceptual
    "The command is mkdir.",                          // identification
    "Give it the name you want, then check with ls."   // near-complete
  ],
  "suggestedSolution": "mkdir reports",
  "alternateSolutions": ["mkdir -p ~/reports", "install -d /home/student/reports"],
  "knownIncorrectSolution": "touch reports"
}
```

### `alternateSolutions` and `knownIncorrectSolution` are not optional in practice

`scripts/test-solutions.ts` runs every solution against a real guest and asserts the positives
pass and the negative fails. The negative is the one that matters: it catches a validator so loose
that a learner passes without doing the task. A task with no `knownIncorrectSolution` has no such
safety net.

### The hint ladder

Conceptual → identification → syntax → near-complete. The worked solution sits behind the last
hint and is tracked separately, because revealing it changes the mastery outcome even when no hints
were opened.

**The last hint must not be the solution.** The validator rejects a hint identical to
`suggestedSolution`, and it caught eleven of these in the first fifteen lessons written — every one
of which would have let a learner reach the answer through the hint ladder while still being scored
as though they had not seen it.

## Validators

70 are available across filesystem, process, service, identity, network, script and package
categories. Read [`lessons/schema/validators.json`](../../lessons/schema/validators.json): it is
the authoritative list, with every parameter and a summary of each.

Useful patterns:

**Compare permissions through a mask** when the task cares about some bits and not others. This is
how "the group must not be able to write" is expressed without dictating every other bit:

```jsonc
{ "type": "file_mode", "path": "/etc/app/config.env", "mode": "0000", "mask": "0007" }
```

**Weight the checks** so partial completion is reported honestly. A task at 75% tells the learner
they are nearly there; a flat failure does not:

```jsonc
{ "type": "directory_exists", "path": "/home/student/reports", "weight": 3 },
{ "type": "file_exists", "path": "/home/student/reports/january.txt" }
```

**Override the message** where the generated one is not specific enough:

```jsonc
{
  "type": "file_contains",
  "path": "/home/student/report.txt",
  "text": "total",
  "message": "report.txt does not contain a total line. Did the pipeline's last stage run?"
}
```

**Guard against hard-coded answers** with a hidden fixture. This is what stops a learner passing a
pipeline lesson by printing the expected output:

```jsonc
{ "type": "unit_test_passes", "path": "/home/student/count-failures.sh", "fixture": "auth-logs" }
```

Fixtures live in `lessons/fixtures/<lesson-id>/fixtures/<fixture>/case-*/`, each case holding an
`input/` directory, an optional `args` file and `expected_stdout`. They are installed mode `0700`
owned by root, so the learner cannot read the cases they are graded against.

## Environment setup

If a lesson needs files to exist before the learner starts, add a setup script at
`lessons/assets/setup/<lesson-id>/setup.sh` and reference it:

```jsonc
"environment": {
  "setupScript": "setup.sh",
  "resetScript": "setup.sh"
}
```

**Make it idempotent** and point `resetScript` at the same file. It runs on prepare *and* on reset,
and the validator warns about a `per-attempt` lesson with a setup script but no reset script —
because Reset task would otherwise be unable to restore what setup created.

Setup scripts run as root in the guest. They must not depend on anything outside the guest, and
they should clear artefacts from a previous attempt so the checks start from a known state.

## What the validator rejects

Errors, all of which would reach a learner as a broken lesson:

- The JSON does not match the schema
- `id` disagrees with the file name, or `module` disagrees with the manifest that lists it
- A lesson file is not listed in its module's manifest
- A prerequisite does not exist, or the prerequisites form a cycle
- A prerequisite comes later in the guided path, so it could never be available in time
- A validator is unknown, unimplemented, missing a required parameter, or given a misspelled one
- A task has no validators
- A `mistake` task has no `brokenCommand` or no `diagnosis`
- A `guided`, `independent` or `applied` task has no `suggestedSolution`
- A hint is the suggested solution verbatim
- An assessment lesson carries hints
- A referenced setup script, fixture or markdown file does not exist
- A validator uses a fixture the lesson does not declare, so the guest would not install it
- A non-concept lesson has no required tasks, so it can never be completed

Warnings worth acting on:

- A required task with no hints, leaving a stuck learner nothing between the task and the answer
- A `per-attempt` lesson with a setup script but no reset script
- A lesson listing no commands, so it will not appear in the command reference

## Before you commit

```bash
npm run lessons:validate     # always
npm run lessons:solutions    # against a running lab, when you can
```

The second needs a live guest:

```bash
LINUXLAB_AGENT_PORT=45003 \
LINUXLAB_CONTROL_TOKEN=<token from the Environment panel> \
LINUXLAB_ONLY_LESSON=filesystem.04 \
npm run lessons:solutions
```
