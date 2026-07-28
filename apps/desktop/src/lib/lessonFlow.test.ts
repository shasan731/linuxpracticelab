import { describe, expect, it } from 'vitest';
import {
  canCheck,
  completedCount,
  currentTask,
  emptyTaskProgress,
  initialProgress,
  isLessonComplete,
  masteryLabel,
  mistakePrompt,
  needsRevisiting,
  requiredTasks,
  solutionIsNext,
  taskPosition,
} from './lessonFlow';
import type { LessonView, TaskView } from './types';

function task(id: string, overrides: Partial<TaskView> = {}): TaskView {
  return {
    id,
    kind: 'guided',
    instruction: `Do ${id}`,
    hintCount: 2,
    optional: false,
    requirements: [],
    ...overrides,
  };
}

function lesson(tasks: TaskView[]): LessonView {
  return {
    id: 'm.01',
    title: 'Fixture lesson',
    level: 'beginner',
    module: 'm',
    type: 'guided-practice',
    estimatedDifficulty: 1,
    prerequisites: [],
    concepts: [],
    commands: [],
    environment: {
      profile: 'filesystem-basic',
      resetPolicy: 'per-attempt',
      networkMode: 'disabled',
      sudoAllowed: false,
    },
    purpose: 'p',
    mentalModel: 'm',
    syntax: [],
    demonstration: [],
    tasks,
    reviewQuestions: [],
    hintsAvailable: true,
  };
}

describe('currentTask', () => {
  it('starts on the first task', () => {
    const l = lesson([task('task-1'), task('task-2')]);
    expect(currentTask(l, initialProgress(l))?.id).toBe('task-1');
  });

  it('advances once a task passes', () => {
    const l = lesson([task('task-1'), task('task-2')]);
    const progress = initialProgress(l);
    progress['task-1'] = { ...emptyTaskProgress(), passed: true };
    expect(currentTask(l, progress)?.id).toBe('task-2');
  });

  it('skips optional tasks while required ones remain', () => {
    const l = lesson([task('task-1', { optional: true }), task('task-2')]);
    expect(currentTask(l, initialProgress(l))?.id).toBe('task-2');
  });

  it('offers a remaining optional task once the required ones are done', () => {
    const l = lesson([task('task-1'), task('task-2', { optional: true })]);
    const progress = initialProgress(l);
    progress['task-1'] = { ...emptyTaskProgress(), passed: true };
    expect(currentTask(l, progress)?.id).toBe('task-2');
  });

  it('returns nothing when everything is finished', () => {
    const l = lesson([task('task-1')]);
    const progress = initialProgress(l);
    progress['task-1'] = { ...emptyTaskProgress(), passed: true };
    expect(currentTask(l, progress)).toBeUndefined();
  });
});

describe('isLessonComplete', () => {
  it('ignores optional tasks', () => {
    const l = lesson([task('task-1'), task('task-2', { optional: true })]);
    const progress = initialProgress(l);
    progress['task-1'] = { ...emptyTaskProgress(), passed: true };
    expect(isLessonComplete(l, progress)).toBe(true);
  });

  it('is false while a required task is outstanding', () => {
    const l = lesson([task('task-1'), task('task-2')]);
    const progress = initialProgress(l);
    progress['task-1'] = { ...emptyTaskProgress(), passed: true };
    expect(isLessonComplete(l, progress)).toBe(false);
  });

  it('treats a lesson with no tasks as complete', () => {
    // Concept lessons have nothing to check, and must not be permanently incomplete.
    const l = lesson([]);
    expect(isLessonComplete(l, {})).toBe(true);
    expect(requiredTasks(l)).toHaveLength(0);
  });

  it('does not crash on progress that is missing an entry', () => {
    const l = lesson([task('task-1')]);
    expect(isLessonComplete(l, {})).toBe(false);
  });
});

describe('counters', () => {
  it('counts passed tasks including optional ones', () => {
    const l = lesson([task('task-1'), task('task-2', { optional: true })]);
    const progress = initialProgress(l);
    progress['task-1'] = { ...emptyTaskProgress(), passed: true };
    progress['task-2'] = { ...emptyTaskProgress(), passed: true };
    expect(completedCount(l, progress)).toBe(2);
  });

  it('reports a one-based position for the header', () => {
    const l = lesson([task('task-1'), task('task-2'), task('task-3')]);
    expect(taskPosition(l, 'task-2')).toEqual({ index: 2, total: 3 });
  });

  it('does not report a negative position for an unknown task', () => {
    const l = lesson([task('task-1')]);
    expect(taskPosition(l, 'nope').index).toBe(0);
  });
});

describe('hints', () => {
  it('offers the solution only once every hint is used', () => {
    const t = task('task-1', { hintCount: 2 });
    expect(solutionIsNext(t, { ...emptyTaskProgress(), hintsRevealed: 0 })).toBe(false);
    expect(solutionIsNext(t, { ...emptyTaskProgress(), hintsRevealed: 1 })).toBe(false);
    expect(solutionIsNext(t, { ...emptyTaskProgress(), hintsRevealed: 2 })).toBe(true);
  });

  it('never offers a solution for a task with no hints', () => {
    // Assessment tasks report hintCount 0, and must not expose a solution button.
    const t = task('task-1', { hintCount: 0 });
    expect(solutionIsNext(t, { ...emptyTaskProgress(), hintsRevealed: 0 })).toBe(false);
  });
});

describe('canCheck', () => {
  const t = task('task-1');

  it('requires a running virtual machine', () => {
    expect(canCheck({ vmReady: false, task: t, progress: emptyTaskProgress(), busy: false })).toBe(false);
  });

  it('is disabled while a check is in flight', () => {
    expect(canCheck({ vmReady: true, task: t, progress: emptyTaskProgress(), busy: true })).toBe(false);
  });

  it('is disabled once the task has passed', () => {
    expect(
      canCheck({
        vmReady: true,
        task: t,
        progress: { ...emptyTaskProgress(), passed: true },
        busy: false,
      }),
    ).toBe(false);
  });

  it('is enabled for an outstanding task on a ready machine', () => {
    expect(canCheck({ vmReady: true, task: t, progress: emptyTaskProgress(), busy: false })).toBe(true);
  });
});

describe('presentation helpers', () => {
  it('labels every mastery level and the unattempted case', () => {
    expect(masteryLabel('mastered')).toBe('Mastered');
    expect(masteryLabel('review-required')).toBe('Review required');
    expect(masteryLabel(undefined)).toBe('Not attempted');
  });

  it('agrees with the Rust definition of a weak lesson', () => {
    expect(needsRevisiting('needs-review')).toBe(true);
    expect(needsRevisiting('review-required')).toBe(true);
    expect(needsRevisiting('passed')).toBe(false);
    expect(needsRevisiting(undefined)).toBe(false);
  });

  it('builds a prompt only for mistake tasks that carry a broken command', () => {
    expect(mistakePrompt(task('task-1', { kind: 'mistake', brokenCommand: 'mkdir a/b' }))).toBe(
      'Someone ran: mkdir a/b',
    );
    expect(mistakePrompt(task('task-1', { kind: 'mistake' }))).toBeUndefined();
    expect(mistakePrompt(task('task-1', { brokenCommand: 'mkdir a/b' }))).toBeUndefined();
  });
});
