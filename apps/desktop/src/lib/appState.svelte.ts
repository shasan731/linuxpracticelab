// Application store, using Svelte 5 runes.
//
// One shared instance holds the bootstrap payload, the current route and the virtual machine
// status. Components read it and call its methods; nothing else owns mutable app state.

import { api, IpcError } from './ipc';
import type { Bootstrap, LessonView, ProgressionMode, VmStatus } from './types';

export type Route =
  | { name: 'home' }
  | { name: 'learn' }
  | { name: 'lesson'; lessonId: string }
  | { name: 'practice' }
  | { name: 'scenarios' }
  | { name: 'reference' }
  | { name: 'progress' }
  | { name: 'achievements' }
  | { name: 'settings' }
  | { name: 'help' };

class AppStore {
  bootstrap = $state<Bootstrap | undefined>(undefined);
  route = $state<Route>({ name: 'home' });
  vm = $state<VmStatus | undefined>(undefined);
  /// Non-fatal problems worth telling the learner about, newest first.
  notices = $state<{ id: number; text: string; tone: 'info' | 'error' }[]>([]);
  loading = $state(true);
  startupError = $state<string | undefined>(undefined);

  #noticeId = 0;

  get vmReady(): boolean {
    return this.vm?.state === 'ready';
  }

  get mode(): ProgressionMode {
    return this.bootstrap?.mode ?? 'guided-path';
  }

  async load(): Promise<void> {
    this.loading = true;
    try {
      await api.installRuntime();
      const bootstrap = await api.bootstrap();
      this.bootstrap = bootstrap;
      this.vm = bootstrap.vm;
      for (const warning of bootstrap.catalogWarnings) {
        this.notify(warning, 'info');
      }
      this.startupError = undefined;
    } catch (error) {
      // A failed bootstrap means the app cannot function, so it is shown in place of the UI
      // rather than as a dismissible notice.
      this.startupError = describeError(error);
    } finally {
      this.loading = false;
    }
  }

  /// Reloads progress-derived data after a lesson result, without disturbing the route.
  async refresh(): Promise<void> {
    try {
      const bootstrap = await api.bootstrap();
      this.bootstrap = bootstrap;
    } catch (error) {
      this.notify(describeError(error), 'error');
    }
  }

  navigate(route: Route): void {
    this.route = route;
  }

  notify(text: string, tone: 'info' | 'error' = 'info'): void {
    this.#noticeId += 1;
    this.notices = [{ id: this.#noticeId, text, tone }, ...this.notices].slice(0, 5);
  }

  dismiss(id: number): void {
    this.notices = this.notices.filter((notice) => notice.id !== id);
  }

  async setMode(mode: ProgressionMode): Promise<void> {
    try {
      await api.setProgressionMode(mode);
      if (this.bootstrap) this.bootstrap.mode = mode;
    } catch (error) {
      this.notify(describeError(error), 'error');
    }
  }

  async startSession(lessonId?: string): Promise<boolean> {
    try {
      this.vm = { ...(this.vm ?? placeholderStatus()), state: 'starting' };
      this.vm = await api.startSession(lessonId);
      return true;
    } catch (error) {
      this.notify(describeError(error), 'error');
      try {
        this.vm = await api.vmStatus();
      } catch {
        // Leave the last known status in place; the notice already explains the failure.
      }
      return false;
    }
  }

  async stopSession(): Promise<void> {
    try {
      await api.stopSession();
      this.vm = await api.vmStatus();
    } catch (error) {
      this.notify(describeError(error), 'error');
    }
  }

  async pollVm(): Promise<void> {
    try {
      this.vm = await api.vmStatus();
    } catch {
      // Polling failures are noise; the next poll either works or the UI already shows a
      // failed state.
    }
  }

  findLesson(lessonId: string): { title: string; module: string } | undefined {
    for (const module of this.bootstrap?.modules ?? []) {
      const lesson = module.lessons.find((entry) => entry.id === lessonId);
      if (lesson) return { title: lesson.title, module: module.title };
    }
    return undefined;
  }
}

function placeholderStatus(): VmStatus {
  return { state: 'stopped', accel: 'tcg', machine: 'microvm', memoryMb: 256 };
}

export function describeError(error: unknown): string {
  if (error instanceof IpcError) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

export const app = new AppStore();

/// Loads a lesson, mapping a failure onto a notice so callers get a definite answer.
export async function loadLesson(lessonId: string): Promise<LessonView | undefined> {
  try {
    return await api.getLesson(lessonId);
  } catch (error) {
    app.notify(describeError(error), 'error');
    return undefined;
  }
}
