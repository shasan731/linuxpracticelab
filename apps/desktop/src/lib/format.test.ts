import { describe, expect, it } from 'vitest';
import {
  commandFromPromptLine,
  failureCategoryLabel,
  formatBytes,
  formatDuration,
  formatKibibytes,
  formatTimestamp,
  percent,
  truncate,
  vmStateLabel,
  vmStateTone,
} from './format';

describe('formatDuration', () => {
  it('formats seconds, minutes and hours', () => {
    expect(formatDuration(45)).toBe('45s');
    expect(formatDuration(90)).toBe('1m 30s');
    expect(formatDuration(3700)).toBe('1h 1m');
  });

  it('handles zero and nonsense input without throwing', () => {
    expect(formatDuration(0)).toBe('0s');
    expect(formatDuration(-5)).toBe('0s');
    expect(formatDuration(Number.NaN)).toBe('0s');
  });
});

describe('formatBytes', () => {
  it('scales through the units', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(2048)).toBe('2.0 kB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatBytes(2 * 1024 * 1024 * 1024)).toBe('2.0 GB');
  });

  it('drops the decimal once the number is large enough not to need it', () => {
    expect(formatBytes(64 * 1024)).toBe('64 kB');
  });

  it('converts kibibytes as reported by the guest', () => {
    // /proc/meminfo reports kB, so 262144 kB is the default 256 MB guest.
    expect(formatKibibytes(262144)).toBe('256 MB');
  });

  it('handles zero and negatives', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(-1)).toBe('0 B');
  });
});

describe('virtual machine status wording', () => {
  it('avoids jargon a beginner would not know', () => {
    const labels = (
      [
        'stopped',
        'starting',
        'booting-guest',
        'ready',
        'paused',
        'stopping',
        'unbootable',
        'failed',
      ] as const
    ).map(vmStateLabel);

    for (const label of labels) {
      expect(label).not.toMatch(/hypervisor|qemu|qcow|overlay|virtio/i);
      expect(label.length).toBeGreaterThan(0);
    }
  });

  it('maps states onto tones for the status indicator', () => {
    expect(vmStateTone('ready')).toBe('good');
    expect(vmStateTone('booting-guest')).toBe('busy');
    expect(vmStateTone('unbootable')).toBe('bad');
    expect(vmStateTone('stopped')).toBe('idle');
  });
});

describe('failureCategoryLabel', () => {
  it('turns a snake_case category into a readable heading', () => {
    expect(failureCategoryLabel('wrong_working_directory')).toBe('Wrong working directory');
    expect(failureCategoryLabel('dns_failure')).toBe('Dns failure');
  });

  it('returns nothing when there is no category', () => {
    expect(failureCategoryLabel(undefined)).toBe('');
  });
});

describe('percent', () => {
  it('rounds and guards against dividing by zero', () => {
    expect(percent(1, 3)).toBe(33);
    expect(percent(24, 201)).toBe(12);
    expect(percent(5, 0)).toBe(0);
  });
});

describe('truncate', () => {
  it('leaves short text alone and does not split characters', () => {
    expect(truncate('short', 10)).toBe('short');
    expect(truncate('abcdefghij', 5)).toBe('abcde…');
    // Astral-plane characters must not be cut in half.
    const emoji = '👍👍👍👍';
    expect(Array.from(truncate(emoji, 2))).toHaveLength(3);
  });
});

describe('commandFromPromptLine', () => {
  it('extracts the command from the lab prompt', () => {
    expect(commandFromPromptLine('student@linuxlab:~$ ls -la')).toBe('ls -la');
    expect(commandFromPromptLine('student@linuxlab:/var/log$ grep error syslog')).toBe(
      'grep error syslog',
    );
  });

  it('handles the root prompt', () => {
    expect(commandFromPromptLine('root@linuxlab:/# systemctl status nginx')).toBe(
      'systemctl status nginx',
    );
  });

  it('ignores output lines and empty prompts', () => {
    expect(commandFromPromptLine('total 8')).toBeUndefined();
    expect(commandFromPromptLine('drwxr-xr-x 2 student student 4096 reports')).toBeUndefined();
    expect(commandFromPromptLine('student@linuxlab:~$ ')).toBeUndefined();
    expect(commandFromPromptLine('student@linuxlab:~$')).toBeUndefined();
  });
});

describe('formatTimestamp', () => {
  it('reports never for an absent time', () => {
    expect(formatTimestamp(0)).toBe('Never');
    expect(formatTimestamp(Number.NaN)).toBe('Never');
  });

  it('renders a real timestamp', () => {
    expect(formatTimestamp(1_700_000_000)).not.toBe('Never');
  });
});
