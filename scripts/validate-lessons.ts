#!/usr/bin/env tsx
/**
 * Validates every lesson package in the repository.
 *
 * This is the gate that stops a broken lesson reaching a learner. It checks four separate
 * things, because each of them fails in a different and independently painful way:
 *
 *   1. Structure   — the JSON matches lessons/schema/lesson.schema.json.
 *   2. Validators  — every validator names a real, implemented check and passes it the
 *                    parameters it requires. This mirrors the same registry the guest agent
 *                    compiles in, so authoring and execution cannot disagree.
 *   3. References  — prerequisites resolve, there are no cycles, and every setup script,
 *                    fixture and markdown file a lesson mentions actually exists.
 *   4. Teaching    — hints progress, solutions exist where the schema cannot enforce it, and
 *                    a lesson that can never be completed is reported rather than shipped.
 *
 * Run with: npm run lessons:validate
 */

import { readdirSync, readFileSync, statSync, existsSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020, { type ErrorObject } from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const scriptDir = fileURLToPath(new URL('.', import.meta.url));
const repoRoot = resolve(scriptDir, '..');
const lessonsRoot = join(repoRoot, 'lessons');
const schemaDir = join(lessonsRoot, 'schema');

// ---------------------------------------------------------------------------
// Types mirroring the schema, kept loose where the schema is the authority.
// ---------------------------------------------------------------------------

interface ParamSpec {
  type: string;
  required?: boolean;
  values?: string[];
  summary?: string;
}

interface ValidatorSpec {
  category: string;
  implemented: boolean;
  failureCategory: string;
  summary: string;
  params?: Record<string, ParamSpec>;
  requiresOneOf?: string[];
}

interface Registry {
  schemaVersion: number;
  failureCategories: string[];
  commonParams: Record<string, ParamSpec>;
  validators: Record<string, ValidatorSpec>;
}

interface Validator {
  type: string;
  [key: string]: unknown;
}

interface Task {
  id: string;
  kind: string;
  instruction: string;
  brokenCommand?: string;
  diagnosis?: string;
  validators: Validator[];
  hints?: string[];
  suggestedSolution?: string;
  knownIncorrectSolution?: string;
  alternateSolutions?: string[];
  optional?: boolean;
}

interface Lesson {
  schemaVersion: number;
  id: string;
  title: string;
  level: string;
  module: string;
  type: string;
  prerequisites?: string[];
  concepts: string[];
  commands?: string[];
  environment: {
    profile: string;
    resetPolicy: string;
    networkMode: string;
    sudoAllowed: boolean;
    namespaces?: string[];
    setupScript?: string;
    resetScript?: string;
    fixtures?: string[];
  };
  content: {
    purpose: string;
    mentalModel: string;
    explanationMarkdown?: string;
  };
  tasks?: Task[];
  reviewQuestions?: unknown[];
}

interface ModuleManifest {
  schemaVersion: number;
  id: string;
  number: number;
  title: string;
  level: string;
  summary: string;
  pack?: string;
  lessons: string[];
}

// ---------------------------------------------------------------------------
// Problem collection
// ---------------------------------------------------------------------------

type Level = 'error' | 'warning';

interface Problem {
  level: Level;
  where: string;
  message: string;
}

const problems: Problem[] = [];

function fail(where: string, message: string): void {
  problems.push({ level: 'error', where, message });
}

function warn(where: string, message: string): void {
  problems.push({ level: 'warning', where, message });
}

function readJson<T>(path: string): T {
  try {
    return JSON.parse(readFileSync(path, 'utf8')) as T;
  } catch (error) {
    fail(relative(repoRoot, path), `is not valid JSON: ${(error as Error).message}`);
    throw error;
  }
}

// ---------------------------------------------------------------------------
// Validator registry checks — the TypeScript twin of crates/shared-types/src/registry.rs
// ---------------------------------------------------------------------------

const registry = readJson<Registry>(join(schemaDir, 'validators.json'));

function checkValidator(where: string, validator: Validator, depth = 0): void {
  if (depth > 3) {
    fail(where, 'validators are nested more than three deep, which is almost certainly a mistake');
    return;
  }

  const spec = registry.validators[validator.type];
  if (!spec) {
    fail(where, `unknown validator '${validator.type}'`);
    return;
  }
  if (!spec.implemented) {
    fail(
      where,
      `validator '${validator.type}' is declared in the registry but not implemented, so the ` +
        `agent would reject it at run time`,
    );
    return;
  }

  const params = spec.params ?? {};

  for (const [name, param] of Object.entries(params)) {
    if (param.required && !(name in validator)) {
      fail(where, `validator '${validator.type}' requires parameter '${name}'`);
    }
  }

  if (spec.requiresOneOf && spec.requiresOneOf.length > 0) {
    const satisfied = spec.requiresOneOf.some((name) => name in validator);
    if (!satisfied) {
      fail(
        where,
        `validator '${validator.type}' needs at least one of: ${spec.requiresOneOf.join(', ')}`,
      );
    }
  }

  for (const [name, value] of Object.entries(validator)) {
    if (name === 'type') continue;
    const param = params[name] ?? registry.commonParams[name];
    if (!param) {
      // A silently ignored parameter is worse than an error: the check would pass for the
      // wrong reason, which is how a task ends up accepting anything.
      fail(where, `validator '${validator.type}' does not accept parameter '${name}'`);
      continue;
    }
    checkParamValue(where, validator.type, name, param, value, depth);
  }
}

function checkParamValue(
  where: string,
  kind: string,
  name: string,
  spec: ParamSpec,
  value: unknown,
  depth: number,
): void {
  const wrong = (expected: string) =>
    fail(where, `validator '${kind}' parameter '${name}' must be ${expected}`);

  switch (spec.type) {
    case 'int':
      if (typeof value !== 'number' || !Number.isInteger(value)) wrong('an integer');
      break;
    case 'port':
      if (typeof value !== 'number' || !Number.isInteger(value) || value < 1 || value > 65535) {
        wrong('a port between 1 and 65535');
      }
      break;
    case 'bool':
      if (typeof value !== 'boolean') wrong('a boolean');
      break;
    case 'list<string>':
      if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
        wrong('an array of strings');
      }
      break;
    case 'validators':
      if (!Array.isArray(value)) {
        wrong('an array of validators');
      } else {
        for (const nested of value) {
          if (typeof nested !== 'object' || nested === null || !('type' in nested)) {
            wrong('an array of validators');
          } else {
            checkValidator(where, nested as Validator, depth + 1);
          }
        }
      }
      break;
    case 'enum':
      if (typeof value !== 'string' || !(spec.values ?? []).includes(value)) {
        wrong(`one of: ${(spec.values ?? []).join(', ')}`);
      }
      break;
    case 'mode':
      if (typeof value === 'string') {
        if (!/^0?o?[0-7]{3,4}$/.test(value)) wrong('an octal mode such as "0644"');
      } else if (typeof value !== 'number') {
        wrong('an octal mode such as "0644"');
      }
      break;
    case 'sha256':
      if (typeof value !== 'string' || !/^[0-9a-f]{64}$/i.test(value)) {
        wrong('a 64 character hex SHA-256 digest');
      }
      break;
    case 'path':
      if (typeof value !== 'string' || value.length === 0) wrong('a non-empty path');
      break;
    case 'regex':
      if (typeof value !== 'string') {
        wrong('a string');
      } else {
        // The agent uses Rust's regex crate, which supports inline flag groups such as (?i)
        // that JavaScript does not. Those are translated to JavaScript flags before testing so
        // a perfectly good Rust pattern is not reported as broken.
        const inline = /^\(\?([imsxU]+)\)/.exec(value);
        const body = inline ? value.slice(inline[0].length) : value;
        const flags = (inline?.[1] ?? '')
          .split('')
          .filter((flag) => 'ims'.includes(flag))
          .join('');
        try {
          new RegExp(body, flags);
        } catch {
          fail(where, `validator '${kind}' pattern '${value}' is not a valid regular expression`);
        }
      }
      break;
    default:
      if (typeof value !== 'string') wrong('a string');
      break;
  }
}

// ---------------------------------------------------------------------------
// Schema setup
// ---------------------------------------------------------------------------

const ajv = new Ajv2020({ allErrors: true, strict: false });
addFormats(ajv);
const validateLesson = ajv.compile(readJson<object>(join(schemaDir, 'lesson.schema.json')));
const validateModule = ajv.compile(readJson<object>(join(schemaDir, 'module.schema.json')));

function renderAjvErrors(errors: ErrorObject[] | null | undefined): string[] {
  return (errors ?? []).map((error) => {
    const path = error.instancePath || '(root)';
    return `${path} ${error.message ?? 'is invalid'}`;
  });
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

function directoriesIn(path: string): string[] {
  if (!existsSync(path)) return [];
  return readdirSync(path)
    .map((name) => join(path, name))
    .filter((candidate) => statSync(candidate).isDirectory());
}

const lessons = new Map<string, { lesson: Lesson; path: string }>();
const modules: { manifest: ModuleManifest; dir: string; path: string }[] = [];

for (const pack of ['core', 'optional']) {
  for (const moduleDir of directoriesIn(join(lessonsRoot, pack))) {
    const manifestPath = join(moduleDir, 'module.json');
    if (!existsSync(manifestPath)) {
      warn(relative(repoRoot, moduleDir), 'has no module.json and will be ignored at run time');
      continue;
    }
    const manifest = readJson<ModuleManifest>(manifestPath);
    const where = relative(repoRoot, manifestPath);

    if (!validateModule(manifest)) {
      for (const message of renderAjvErrors(validateModule.errors)) fail(where, message);
    }
    const declaredPack = manifest.pack ?? 'core';
    if (declaredPack !== pack) {
      fail(where, `declares pack '${declaredPack}' but lives under lessons/${pack}`);
    }
    modules.push({ manifest, dir: moduleDir, path: manifestPath });

    // Lesson files that exist but are not listed are invisible to the application, which is
    // exactly the kind of silent omission this script exists to catch.
    const listed = new Set(manifest.lessons);
    for (const entry of readdirSync(moduleDir)) {
      if (!entry.endsWith('.json') || entry === 'module.json') continue;
      const id = entry.slice(0, -'.json'.length);
      if (!listed.has(id)) {
        fail(
          relative(repoRoot, join(moduleDir, entry)),
          `is not listed in ${manifest.id}/module.json, so it would never be shown`,
        );
      }
    }

    for (const lessonId of manifest.lessons) {
      const lessonPath = join(moduleDir, `${lessonId}.json`);
      if (!existsSync(lessonPath)) {
        fail(where, `lists lesson '${lessonId}' but ${lessonId}.json does not exist`);
        continue;
      }
      const lesson = readJson<Lesson>(lessonPath);
      const lessonWhere = relative(repoRoot, lessonPath);

      if (lessons.has(lessonId)) {
        fail(lessonWhere, `duplicate lesson id '${lessonId}'`);
        continue;
      }
      if (lesson.id !== lessonId) {
        fail(lessonWhere, `declares id '${lesson.id}' but its file name says '${lessonId}'`);
      }
      if (lesson.module !== manifest.id) {
        fail(
          lessonWhere,
          `says it belongs to module '${lesson.module}' but is listed by '${manifest.id}'`,
        );
      }
      lessons.set(lessonId, { lesson, path: lessonPath });
    }
  }
}

if (modules.length === 0) {
  fail('lessons/', 'no modules were found');
}

// ---------------------------------------------------------------------------
// Per-lesson checks
// ---------------------------------------------------------------------------

const assessmentTypes = new Set(['assessment', 'capstone']);

for (const [lessonId, { lesson, path }] of lessons) {
  const where = relative(repoRoot, path);

  if (!validateLesson(lesson)) {
    for (const message of renderAjvErrors(validateLesson.errors)) fail(where, message);
  }

  // Prerequisites.
  for (const prerequisite of lesson.prerequisites ?? []) {
    if (prerequisite === lessonId) {
      fail(where, 'lists itself as a prerequisite');
    } else if (!lessons.has(prerequisite)) {
      fail(where, `prerequisite '${prerequisite}' does not exist`);
    }
  }

  // Referenced files must exist, or the lesson fails when a learner opens it rather than now.
  const setupDir = join(lessonsRoot, 'assets', 'setup', lessonId);
  for (const [field, script] of [
    ['setupScript', lesson.environment.setupScript],
    ['resetScript', lesson.environment.resetScript],
  ] as const) {
    if (!script) continue;
    const scriptPath = join(setupDir, script);
    if (!existsSync(scriptPath)) {
      fail(
        where,
        `${field} '${script}' is missing: expected lessons/assets/setup/${lessonId}/${script}`,
      );
    }
  }

  for (const fixture of lesson.environment.fixtures ?? []) {
    const fixturePath = join(lessonsRoot, 'fixtures', lessonId, 'fixtures', fixture);
    const alternatePath = join(lessonsRoot, 'fixtures', lessonId, fixture);
    if (!existsSync(fixturePath) && !existsSync(alternatePath)) {
      fail(where, `fixture '${fixture}' is missing under lessons/fixtures/${lessonId}/`);
    }
  }

  if (lesson.content.explanationMarkdown) {
    const markdownPath = join(lessonsRoot, lesson.content.explanationMarkdown);
    if (!existsSync(markdownPath)) {
      fail(where, `explanationMarkdown '${lesson.content.explanationMarkdown}' does not exist`);
    }
  }

  // A lesson whose reset policy is per-attempt but which has no reset script cannot actually
  // reset anything it created, so a second attempt starts from a dirty state.
  if (
    lesson.environment.resetPolicy === 'per-attempt' &&
    lesson.environment.setupScript &&
    !lesson.environment.resetScript
  ) {
    warn(
      where,
      'has a setup script and resetPolicy per-attempt but no resetScript, so Reset task cannot ' +
        'restore what setup created',
    );
  }

  const isAssessment = assessmentTypes.has(lesson.type);
  const tasks = lesson.tasks ?? [];
  const requiredTasks = tasks.filter((task) => !task.optional);

  // Concept lessons legitimately have no tasks; anything else needs at least one.
  if (requiredTasks.length === 0 && !['concept', 'demonstration'].includes(lesson.type)) {
    fail(where, `is a ${lesson.type} lesson with no required tasks, so it can never be completed`);
  }

  const taskIds = new Set<string>();
  const usedFixtures = new Set<string>();

  for (const task of tasks) {
    const taskWhere = `${where} · ${task.id}`;

    if (taskIds.has(task.id)) fail(taskWhere, 'duplicate task id');
    taskIds.add(task.id);

    if (task.validators.length === 0) {
      fail(taskWhere, 'has no validators, so it would pass or fail for no stated reason');
    }
    for (const validator of task.validators) {
      checkValidator(taskWhere, validator);
      const fixture = (validator as Record<string, unknown>).fixture;
      if (typeof fixture === 'string') usedFixtures.add(fixture);
    }

    if (task.kind === 'mistake') {
      if (!task.brokenCommand) fail(taskWhere, 'is a mistake task with no brokenCommand');
      if (!task.diagnosis) fail(taskWhere, 'is a mistake task with no diagnosis');
    }

    if (['guided', 'independent', 'applied'].includes(task.kind) && !task.suggestedSolution) {
      fail(taskWhere, `is a ${task.kind} task with no suggestedSolution`);
    }

    // Hints go conceptual first, near-complete last. An assessment must offer none.
    const hints = task.hints ?? [];
    if (isAssessment && hints.length > 0) {
      fail(taskWhere, 'is in an assessment lesson but carries hints, which must not be offered');
    }
    if (!isAssessment && !task.optional && hints.length === 0 && task.kind !== 'mistake') {
      warn(taskWhere, 'has no hints, so a stuck learner has nothing between the task and the answer');
    }
    if (hints.some((hint) => hint.trim().length < 8)) {
      fail(taskWhere, 'has a hint too short to be useful');
    }
    // A hint that simply is the answer defeats the hint ladder and skews mastery scoring.
    if (task.suggestedSolution) {
      const solution = task.suggestedSolution.trim();
      for (const [index, hint] of hints.entries()) {
        if (hint.trim() === solution) {
          fail(taskWhere, `hint ${index + 1} is the suggested solution verbatim`);
        }
      }
    }
  }

  for (const fixture of usedFixtures) {
    if (!(lesson.environment.fixtures ?? []).includes(fixture)) {
      fail(
        where,
        `a validator uses fixture '${fixture}' but it is not declared in environment.fixtures, ` +
          `so the guest would not install it`,
      );
    }
  }

  if (lesson.commands && lesson.commands.length === 0) {
    warn(where, 'lists no commands, so it will not appear in the command reference');
  }
}

// ---------------------------------------------------------------------------
// Prerequisite cycles
// ---------------------------------------------------------------------------

function findCycles(): string[][] {
  const state = new Map<string, 'open' | 'done'>();
  const stack: string[] = [];
  const cycles: string[][] = [];

  function visit(id: string): void {
    const mark = state.get(id);
    if (mark === 'done') return;
    if (mark === 'open') {
      const start = stack.indexOf(id);
      if (start >= 0) cycles.push([...stack.slice(start), id]);
      return;
    }
    state.set(id, 'open');
    stack.push(id);
    for (const prerequisite of lessons.get(id)?.lesson.prerequisites ?? []) {
      if (lessons.has(prerequisite)) visit(prerequisite);
    }
    stack.pop();
    state.set(id, 'done');
  }

  for (const id of lessons.keys()) visit(id);
  return cycles;
}

for (const cycle of findCycles()) {
  fail(cycle[0] ?? 'lessons/', `prerequisite cycle: ${cycle.join(' -> ')}`);
}

// ---------------------------------------------------------------------------
// Curriculum-level checks
// ---------------------------------------------------------------------------

const coreModules = modules.filter((entry) => (entry.manifest.pack ?? 'core') === 'core');
const coreLessonCount = coreModules.reduce((total, entry) => total + entry.manifest.lessons.length, 0);

const seenNumbers = new Map<number, string>();
for (const { manifest, path } of coreModules) {
  const existing = seenNumbers.get(manifest.number);
  if (existing) {
    fail(relative(repoRoot, path), `module number ${manifest.number} is also used by '${existing}'`);
  }
  seenNumbers.set(manifest.number, manifest.id);
}

// The guided path walks core modules in number order, so a lesson must never depend on one
// that comes later.
const orderedCoreLessons: string[] = coreModules
  .slice()
  .sort((a, b) => a.manifest.number - b.manifest.number)
  .flatMap((entry) => entry.manifest.lessons);
const position = new Map(orderedCoreLessons.map((id, index) => [id, index]));

for (const [lessonId, { lesson, path }] of lessons) {
  const own = position.get(lessonId);
  if (own === undefined) continue;
  for (const prerequisite of lesson.prerequisites ?? []) {
    const prerequisitePosition = position.get(prerequisite);
    if (prerequisitePosition !== undefined && prerequisitePosition > own) {
      fail(
        relative(repoRoot, path),
        `depends on '${prerequisite}', which comes later in the guided path and so would never ` +
          `be available in time`,
      );
    }
  }
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

const errors = problems.filter((problem) => problem.level === 'error');
const warnings = problems.filter((problem) => problem.level === 'warning');

const validatorsUsed = new Set<string>();
for (const { lesson } of lessons.values()) {
  for (const task of lesson.tasks ?? []) {
    for (const validator of task.validators) validatorsUsed.add(validator.type);
  }
}

console.log('Linux Practice Lab — lesson validation');
console.log('');
console.log(`  modules          ${modules.length} (${coreModules.length} core)`);
console.log(`  lessons          ${lessons.size} (${coreLessonCount} in the core path)`);
console.log(`  validators used  ${validatorsUsed.size} of ${Object.keys(registry.validators).length} available`);
console.log('');

for (const problem of warnings) {
  console.log(`  warning  ${problem.where}: ${problem.message}`);
}
for (const problem of errors) {
  console.log(`  ERROR    ${problem.where}: ${problem.message}`);
}

if (errors.length === 0) {
  console.log(
    warnings.length === 0
      ? '  Everything checks out.'
      : `  No errors. ${warnings.length} warning(s) to consider.`,
  );
  process.exit(0);
}

console.log('');
console.log(`  ${errors.length} error(s). Fix these before shipping.`);
process.exit(1);
