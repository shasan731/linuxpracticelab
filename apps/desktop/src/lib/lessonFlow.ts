// Lesson player logic, kept free of Svelte and Tauri so it can be unit tested directly.
//
// The player's job is to know which task is current, what the learner has finished, and when
// the lesson is done. Getting that wrong is how a learner ends up stuck on a passed task or
// skipped past one they never completed, so it lives here with tests rather than inline in a
// component.

import type { LessonView, MasteryStatus, TaskView } from './types';

export interface TaskProgress {
  passed: boolean;
  hintsRevealed: number;
  hints: string[];
  solution?: string;
  attempted: boolean;
}

export type TaskProgressMap = Record<string, TaskProgress>;

export function emptyTaskProgress(): TaskProgress {
  return { passed: false, hintsRevealed: 0, hints: [], attempted: false };
}

export function initialProgress(lesson: LessonView): TaskProgressMap {
  const progress: TaskProgressMap = {};
  for (const task of lesson.tasks) {
    progress[task.id] = emptyTaskProgress();
  }
  return progress;
}

/// The first task that is not yet passed, skipping optional ones the learner has left alone.
/// Returns undefined once every required task is done.
export function currentTask(lesson: LessonView, progress: TaskProgressMap): TaskView | undefined {
  const firstUnfinished = lesson.tasks.find((task) => !progress[task.id]?.passed && !task.optional);
  if (firstUnfinished) return firstUnfinished;
  // All required tasks are done; offer any remaining optional ones rather than nothing.
  return lesson.tasks.find((task) => !progress[task.id]?.passed);
}

export function requiredTasks(lesson: LessonView): TaskView[] {
  return lesson.tasks.filter((task) => !task.optional);
}

export function isLessonComplete(lesson: LessonView, progress: TaskProgressMap): boolean {
  const required = requiredTasks(lesson);
  // A lesson with no tasks is a concept lesson: reading it is completing it.
  if (required.length === 0) return true;
  return required.every((task) => progress[task.id]?.passed === true);
}

export function completedCount(lesson: LessonView, progress: TaskProgressMap): number {
  return lesson.tasks.filter((task) => progress[task.id]?.passed).length;
}

/// Position of a task in the lesson, one-based, for the "Progress 3 of 7" header.
export function taskPosition(lesson: LessonView, taskId: string): { index: number; total: number } {
  const index = lesson.tasks.findIndex((task) => task.id === taskId);
  return { index: index < 0 ? 0 : index + 1, total: lesson.tasks.length };
}

/// Whether the next hint request would show the worked solution instead of another hint.
export function solutionIsNext(task: TaskView, progress: TaskProgress | undefined): boolean {
  const revealed = progress?.hintsRevealed ?? 0;
  return task.hintCount > 0 && revealed >= task.hintCount;
}

export function masteryLabel(mastery: MasteryStatus | undefined): string {
  switch (mastery) {
    case 'mastered':
      return 'Mastered';
    case 'strong':
      return 'Strong';
    case 'passed':
      return 'Passed';
    case 'needs-review':
      return 'Needs review';
    case 'review-required':
      return 'Review required';
    default:
      return 'Not attempted';
  }
}

/// Mastery levels worth revisiting. Matches MasteryStatus::needs_revisiting on the Rust side.
export function needsRevisiting(mastery: MasteryStatus | undefined): boolean {
  return mastery === 'needs-review' || mastery === 'review-required';
}

/// The prompt a mistake task shows. A mistake task asks the learner to diagnose a broken
/// command, so the instruction alone is not enough context.
export function mistakePrompt(task: TaskView): string | undefined {
  if (task.kind !== 'mistake' || !task.brokenCommand) return undefined;
  return `Someone ran: ${task.brokenCommand}`;
}

/// Whether the Check button should be enabled. Checking with no VM, or on an already-passed
/// task, is either impossible or pointless.
export function canCheck(options: {
  vmReady: boolean;
  task: TaskView | undefined;
  progress: TaskProgress | undefined;
  busy: boolean;
}): boolean {
  if (!options.vmReady || options.busy || !options.task) return false;
  return options.progress?.passed !== true;
}
