#!/usr/bin/env tsx
/**
 * Runs every lesson's solutions against a real Linux guest and checks the validators agree.
 *
 * Spec 21.2 asks for four things this script provides: the suggested solution passes, every
 * alternate solution passes, a known incorrect solution fails, and the setup and reset scripts
 * succeed. Together they catch the two failure modes that hurt most — a task nobody can pass,
 * and a task that passes without doing anything.
 *
 * It needs a running lab. Start one and export the connection details:
 *
 *   LINUXLAB_AGENT_PORT=45003 LINUXLAB_CONTROL_TOKEN=... npm run lessons:solutions
 *
 * The trick that makes this possible without adding a "run this command" operation to the
 * agent — which would be a remote shell, and therefore a liability — is the side_effect_exists
 * validator. It already runs a command and then applies nested validators to the resulting
 * state, which is exactly the shape of "run the solution, then check the task".
 */

import { readdirSync, readFileSync, existsSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { connect, type Socket } from 'node:net';
import { createInterface } from 'node:readline';

const scriptDir = fileURLToPath(new URL('.', import.meta.url));
const repoRoot = resolve(scriptDir, '..');
const lessonsRoot = join(repoRoot, 'lessons');

const AGENT_PORT = Number(process.env.LINUXLAB_AGENT_PORT ?? '0');
const CONTROL_TOKEN = process.env.LINUXLAB_CONTROL_TOKEN ?? '';
const ONLY = process.env.LINUXLAB_ONLY_LESSON;

if (!AGENT_PORT || !CONTROL_TOKEN) {
  console.error(
    'This script needs a running lab.\n' +
      '  LINUXLAB_AGENT_PORT      loopback port QEMU exposes the agent channel on\n' +
      '  LINUXLAB_CONTROL_TOKEN   the token that VM run was started with\n' +
      '  LINUXLAB_ONLY_LESSON     optional: test a single lesson id',
  );
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Agent client
// ---------------------------------------------------------------------------

type Json = Record<string, unknown>;

class AgentClient {
  #socket: Socket;
  #nextId = 1;
  #pending = new Map<number, (response: Json) => void>();
  #ready: Promise<void>;

  constructor(port: number) {
    this.#socket = connect({ host: '127.0.0.1', port });
    this.#socket.setNoDelay(true);
    this.#ready = new Promise((resolveReady, rejectReady) => {
      this.#socket.once('connect', () => resolveReady());
      this.#socket.once('error', rejectReady);
    });

    const lines = createInterface({ input: this.#socket });
    lines.on('line', (line) => {
      if (!line.trim()) return;
      try {
        const envelope = JSON.parse(line) as { id: number; response: Json };
        this.#pending.get(envelope.id)?.(envelope.response);
        this.#pending.delete(envelope.id);
      } catch (error) {
        console.error(`  malformed reply from the guest: ${(error as Error).message}`);
      }
    });
  }

  async request(request: Json, timeoutMs = 180_000): Promise<Json> {
    await this.#ready;
    const id = this.#nextId++;
    return new Promise((resolveRequest, rejectRequest) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        rejectRequest(new Error(`the guest did not reply within ${timeoutMs}ms`));
      }, timeoutMs);
      this.#pending.set(id, (response) => {
        clearTimeout(timer);
        resolveRequest(response);
      });
      this.#socket.write(`${JSON.stringify({ id, token: CONTROL_TOKEN, request })}\n`);
    });
  }

  close(): void {
    this.#socket.destroy();
  }
}

// ---------------------------------------------------------------------------
// Lesson discovery
// ---------------------------------------------------------------------------

interface Validator {
  type: string;
  [key: string]: unknown;
}

interface Task {
  id: string;
  kind: string;
  validators: Validator[];
  suggestedSolution?: string;
  alternateSolutions?: string[];
  knownIncorrectSolution?: string;
  optional?: boolean;
}

interface Lesson {
  id: string;
  title: string;
  type: string;
  environment: {
    setupScript?: string;
    resetScript?: string;
    fixtures?: string[];
    namespaces?: string[];
    sudoAllowed: boolean;
  };
  tasks?: Task[];
}

function collectLessons(): Lesson[] {
  const found: Lesson[] = [];
  for (const pack of ['core', 'optional']) {
    const packDir = join(lessonsRoot, pack);
    if (!existsSync(packDir)) continue;
    for (const name of readdirSync(packDir)) {
      const moduleDir = join(packDir, name);
      if (!statSync(moduleDir).isDirectory()) continue;
      const manifestPath = join(moduleDir, 'module.json');
      if (!existsSync(manifestPath)) continue;
      const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as { lessons: string[] };
      for (const lessonId of manifest.lessons) {
        const lessonPath = join(moduleDir, `${lessonId}.json`);
        if (!existsSync(lessonPath)) continue;
        found.push(JSON.parse(readFileSync(lessonPath, 'utf8')) as Lesson);
      }
    }
  }
  return ONLY ? found.filter((lesson) => lesson.id === ONLY) : found;
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

interface Failure {
  lesson: string;
  task: string;
  what: string;
  detail: string;
}

const failures: Failure[] = [];
let checksRun = 0;

/** Wraps a command and the task's validators into one side_effect_exists check. */
function solutionProbe(command: string, validators: Validator[]): Validator {
  return {
    type: 'side_effect_exists',
    command,
    then: validators,
    timeoutMs: 30_000,
  };
}

async function validate(
  client: AgentClient,
  lessonId: string,
  taskId: string,
  validators: Validator[],
): Promise<{ passed: boolean; detail: string }> {
  const response = await client.request({
    op: 'validateTask',
    lessonId,
    taskId,
    validators,
    subjectUser: 'student',
  });
  checksRun += 1;

  if (response.kind === 'error') {
    return { passed: false, detail: String(response.message ?? 'agent error') };
  }
  if (response.kind !== 'taskValidated') {
    return { passed: false, detail: `unexpected reply kind '${String(response.kind)}'` };
  }
  const validation = response as unknown as {
    passed: boolean;
    errored: boolean;
    outcomes: { passed: boolean; message: string; errored: boolean }[];
  };
  const problems = (validation.outcomes ?? [])
    .filter((outcome) => !outcome.passed)
    .map((outcome) => outcome.message)
    .join('; ');
  return { passed: validation.passed === true, detail: problems };
}

async function resetLesson(client: AgentClient, lesson: Lesson): Promise<void> {
  const response = await client.request({
    op: 'resetLesson',
    lessonId: lesson.id,
    resetScript: lesson.environment.resetScript ?? null,
  });
  if (response.kind === 'error') {
    failures.push({
      lesson: lesson.id,
      task: '(reset)',
      what: 'reset script failed',
      detail: String(response.message),
    });
  }
}

async function prepareLesson(client: AgentClient, lesson: Lesson): Promise<boolean> {
  const response = await client.request({
    op: 'prepareLesson',
    lessonId: lesson.id,
    setupScript: lesson.environment.setupScript ?? null,
    fixtures: lesson.environment.fixtures ?? [],
    namespaces: lesson.environment.namespaces ?? [],
    sudoAllowed: lesson.environment.sudoAllowed,
  });
  if (response.kind === 'error') {
    failures.push({
      lesson: lesson.id,
      task: '(setup)',
      what: 'setup script failed',
      detail: String(response.message),
    });
    return false;
  }
  for (const warning of (response.warnings as string[]) ?? []) {
    console.log(`    warning: ${warning}`);
  }
  return true;
}

async function testTask(client: AgentClient, lesson: Lesson, task: Task): Promise<void> {
  const candidates = [task.suggestedSolution, ...(task.alternateSolutions ?? [])].filter(
    (candidate): candidate is string => typeof candidate === 'string' && candidate.length > 0,
  );

  for (const [index, solution] of candidates.entries()) {
    // A fresh environment per candidate, otherwise the second solution is graded against the
    // state the first one left behind and passes for the wrong reason.
    await prepareLesson(client, lesson);
    await resetLesson(client, lesson);

    const result = await validate(client, lesson.id, task.id, [
      solutionProbe(solution, task.validators),
    ]);
    if (!result.passed) {
      failures.push({
        lesson: lesson.id,
        task: task.id,
        what: index === 0 ? 'suggested solution does not pass' : `alternate solution ${index} does not pass`,
        detail: `${solution} — ${result.detail}`,
      });
    }
  }

  // The negative case is the one that catches a validator which passes trivially.
  if (task.knownIncorrectSolution) {
    await prepareLesson(client, lesson);
    await resetLesson(client, lesson);

    const result = await validate(client, lesson.id, task.id, [
      solutionProbe(task.knownIncorrectSolution, task.validators),
    ]);
    if (result.passed) {
      failures.push({
        lesson: lesson.id,
        task: task.id,
        what: 'known incorrect solution was accepted',
        detail:
          `${task.knownIncorrectSolution} — the validators are too loose, so a learner could ` +
          `pass without doing the task`,
      });
    }
  }

  if (candidates.length === 0 && task.kind !== 'mistake' && !task.optional) {
    failures.push({
      lesson: lesson.id,
      task: task.id,
      what: 'no solution to test',
      detail: 'the task declares neither a suggested nor an alternate solution',
    });
  }
}

async function main(): Promise<void> {
  const lessons = collectLessons();
  const client = new AgentClient(AGENT_PORT);

  const pong = await client.request({ op: 'ping' }, 30_000);
  if (pong.kind !== 'pong') {
    console.error(`the guest did not answer a ping: ${JSON.stringify(pong)}`);
    client.close();
    process.exit(1);
  }
  console.log(
    `Connected to guest image ${String(pong.imageVersion)}, agent ${String(pong.agentVersion)}`,
  );
  console.log(`Testing ${lessons.length} lesson(s)`);
  console.log('');

  for (const lesson of lessons) {
    const tasks = lesson.tasks ?? [];
    if (tasks.length === 0) {
      console.log(`  ${lesson.id}  no tasks, nothing to test`);
      continue;
    }
    console.log(`  ${lesson.id}  ${lesson.title}`);
    const before = failures.length;
    for (const task of tasks) {
      await testTask(client, lesson, task);
    }
    const added = failures.length - before;
    console.log(added === 0 ? '    ok' : `    ${added} problem(s)`);
  }

  client.close();

  console.log('');
  console.log(`${checksRun} validation run(s) against the guest`);

  if (failures.length === 0) {
    console.log('Every solution passes and every known incorrect solution is rejected.');
    process.exit(0);
  }

  console.log('');
  for (const failure of failures) {
    console.log(`  ${failure.lesson} · ${failure.task}: ${failure.what}`);
    console.log(`      ${failure.detail}`);
  }
  console.log('');
  console.log(`${failures.length} problem(s).`);
  process.exit(1);
}

main().catch((error) => {
  console.error(`solution testing could not run: ${(error as Error).message}`);
  process.exit(1);
});
