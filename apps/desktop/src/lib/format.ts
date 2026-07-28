// Presentation helpers. Pure functions, unit tested, so the components stay declarative.

import type { FailureCategory, VmState } from './types';

export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0s';
  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remainder = total % 60;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${remainder}s`;
  return `${remainder}s`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
  const units = ['B', 'kB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // One decimal place above kB; whole bytes have no meaningful fraction.
  const rendered = unit === 0 ? String(Math.round(value)) : value.toFixed(value < 10 ? 1 : 0);
  return `${rendered} ${units[unit]}`;
}

export function formatKibibytes(kb: number): string {
  return formatBytes(kb * 1024);
}

/// What the status bar says about the virtual machine. Written for someone who has never heard
/// the word "hypervisor".
export function vmStateLabel(state: VmState): string {
  switch (state) {
    case 'stopped':
      return 'Linux is not running';
    case 'starting':
      return 'Starting Linux';
    case 'booting-guest':
      return 'Linux is starting up';
    case 'ready':
      return 'Linux is ready';
    case 'paused':
      return 'Paused';
    case 'stopping':
      return 'Shutting down';
    case 'unbootable':
      return 'Environment needs restoring';
    case 'failed':
      return 'Linux could not start';
    default:
      return 'Unknown';
  }
}

export function vmStateTone(state: VmState): 'good' | 'busy' | 'bad' | 'idle' {
  switch (state) {
    case 'ready':
      return 'good';
    case 'starting':
    case 'booting-guest':
    case 'stopping':
      return 'busy';
    case 'failed':
    case 'unbootable':
      return 'bad';
    default:
      return 'idle';
  }
}

/// Human-readable name for a failure category, used as the heading above the guidance.
export function failureCategoryLabel(category: FailureCategory | undefined): string {
  if (!category) return '';
  return category
    .split('_')
    .map((word, index) => (index === 0 ? word.charAt(0).toUpperCase() + word.slice(1) : word))
    .join(' ');
}

/// Formats a Unix timestamp in the learner's locale. Progress is local-only, so local time is
/// the right frame of reference.
export function formatTimestamp(unixSeconds: number): string {
  if (!Number.isFinite(unixSeconds) || unixSeconds <= 0) return 'Never';
  return new Date(unixSeconds * 1000).toLocaleString();
}

export function percent(value: number, total: number): number {
  if (total <= 0) return 0;
  return Math.round((value / total) * 100);
}

/// Truncates for a single-line label without cutting a surrogate pair in half.
export function truncate(text: string, max: number): string {
  const characters = Array.from(text);
  if (characters.length <= max) return text;
  return `${characters.slice(0, max).join('')}…`;
}

/// Extracts the command a learner typed from a terminal line, for the history panel.
/// Returns undefined for anything that is not a prompt line.
export function commandFromPromptLine(line: string): string | undefined {
  // Matches the lab prompt: student@linuxlab:~$ command, or root's # variant.
  const match = /^[a-z_][a-z0-9_-]*@[a-z0-9-]+:[^$#]*[$#]\s?(.*)$/i.exec(line.trimEnd());
  const command = match?.[1]?.trim();
  return command ? command : undefined;
}
