// Typed wrappers around the Tauri command surface.
//
// Every call goes through `call`, which turns a rejected promise into a thrown Error with the
// host's message intact. The host writes those messages for the learner, so they are shown
// verbatim rather than being replaced with something generic.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  Bootstrap,
  CheckResult,
  CommandEntry,
  DirEntryInfo,
  GuestDiagnostics,
  HealthReport,
  HintResponse,
  LessonView,
  ProgressReport,
  ProgressionMode,
  ReviewGrade,
  SnapshotRow,
  SolutionResponse,
  VmStatus,
} from './types';

export class IpcError extends Error {
  constructor(
    message: string,
    readonly command: string,
  ) {
    super(message);
    this.name = 'IpcError';
  }
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    // Tauri rejects with the String our command returned, so preserve it as-is.
    const message = typeof error === 'string' ? error : String(error);
    throw new IpcError(message, command);
  }
}

export const api = {
  installRuntime: (force = false) => call<boolean>('install_runtime', { force }),
  bootstrap: () => call<Bootstrap>('bootstrap'),
  getLesson: (lessonId: string) => call<LessonView>('get_lesson', { lessonId }),
  setProgressionMode: (mode: ProgressionMode) => call<void>('set_progression_mode', { mode }),
  commandReference: () => call<CommandEntry[]>('command_reference'),

  startSession: (lessonId?: string) => call<VmStatus>('start_session', { lessonId: lessonId ?? null }),
  vmStatus: () => call<VmStatus>('vm_status'),
  stopSession: () => call<void>('stop_session'),
  restartVm: () => call<VmStatus>('restart_vm'),

  terminalWrite: (data: Uint8Array) =>
    // Tauri serialises numbers, not typed arrays, so this is converted explicitly.
    call<void>('terminal_write', { data: Array.from(data) }),
  terminalResize: (rows: number, cols: number) => call<void>('terminal_resize', { rows, cols }),
  recordCommand: (command: string) => call<boolean>('record_command', { command }),
  exportTranscript: (transcript: string) => call<string>('export_transcript', { transcript }),

  prepareLesson: (lessonId: string) => call<string[]>('prepare_lesson', { lessonId }),
  checkTask: (lessonId: string, taskId: string) => call<CheckResult>('check_task', { lessonId, taskId }),
  revealHint: (lessonId: string, taskId: string) => call<HintResponse>('reveal_hint', { lessonId, taskId }),
  revealSolution: (lessonId: string, taskId: string) =>
    call<SolutionResponse>('reveal_solution', { lessonId, taskId }),
  gradeReviewQuestion: (
    lessonId: string,
    index: number,
    selected: number[] | null,
    text: string | null,
  ) => call<ReviewGrade>('grade_review_question', { lessonId, index, selected, text }),
  resetLesson: (lessonId: string) => call<void>('reset_lesson', { lessonId }),
  restartLesson: (lessonId: string) => call<VmStatus>('restart_lesson', { lessonId }),

  guestDiagnostics: () => call<GuestDiagnostics>('guest_diagnostics'),
  listDirectory: (path: string, includeHidden: boolean) =>
    call<DirEntryInfo[]>('list_directory', { path, includeHidden }),

  createSnapshot: (name: string) => call<string>('create_snapshot', { name }),
  listSnapshots: () => call<SnapshotRow[]>('list_snapshots'),
  restoreSnapshot: (id: number) => call<void>('restore_snapshot', { id }),
  factoryResetPractice: () => call<void>('factory_reset_practice'),
  verifyRuntime: () => call<string[]>('verify_runtime'),
  healthCheck: () => call<HealthReport>('health_check'),

  progressReport: () => call<ProgressReport>('progress_report'),
  getSetting: (key: string) => call<string | null>('get_setting', { key }),
  setSetting: (key: string, value: string) => call<void>('set_setting', { key, value }),
  bumpPracticeTime: (seconds: number) => call<number>('bump_practice_time', { seconds }),
};

export const TERMINAL_OUTPUT_EVENT = 'terminal://output';
export const TERMINAL_CLOSED_EVENT = 'terminal://closed';

/// Subscribes to guest console output. The payload is a byte array so multi-byte characters
/// split across reads are reassembled by xterm.js rather than mangled here.
export function onTerminalOutput(handler: (bytes: Uint8Array) => void): Promise<UnlistenFn> {
  return listen<number[]>(TERMINAL_OUTPUT_EVENT, (event) => {
    handler(new Uint8Array(event.payload));
  });
}

export function onTerminalClosed(handler: () => void): Promise<UnlistenFn> {
  return listen(TERMINAL_CLOSED_EVENT, () => handler());
}
